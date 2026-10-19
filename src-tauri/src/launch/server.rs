//! Servers a friend can be invited to (D-064): which one the game joined, read
//! from its own log line, and the arguments that start the game straight into
//! one. Addresses come from other players' invites, so they are checked here
//! before they reach a command line: a host name, an IPv4 address or a
//! bracketed IPv6 address, and a port.

/// `play.example.com:25565`, `192.0.2.7:25565` or `[2001:db8::1]:25565`, with
/// the port optional (25565). `None` for anything else.
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
        (host.to_ascii_lowercase(), port)
    };
    (port != 0).then_some((host, port))
}

/// A DNS name (letters, digits, hyphens, dots; labels of 1 to 63) or an IPv4
/// address.
fn valid_host(host: &str) -> bool {
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

/// The server in the game's own line "Connecting to play.example.com, 25565"
/// (in plain or XML log output), as `host:port`.
pub fn connecting_to(line: &str) -> Option<String> {
    const MARK: &str = "Connecting to ";
    let start = line.find(MARK)? + MARK.len();
    let rest = &line[start..];
    let (host, rest) = rest.split_once(", ")?;
    let port: String = rest.chars().take_while(char::is_ascii_digit).collect();
    let (host, port) = parse_address(&format!("{host}:{port}"))?;
    Some(format!("{host}:{port}"))
}

/// Arguments that start the game straight into the server: quick play for
/// 1.20 and later (and snapshots), `--server`/`--port` before. Minecraft
/// ignores options it does not know.
pub fn join_arguments(version_id: &str, host: &str, port: u16) -> Vec<String> {
    if quick_play(version_id) {
        vec!["--quickPlayMultiplayer".into(), format!("{host}:{port}")]
    } else {
        vec![
            "--server".into(),
            host.trim_start_matches('[')
                .trim_end_matches(']')
                .to_owned(),
            "--port".into(),
            port.to_string(),
        ]
    }
}

/// Quick play came with 1.20 (snapshot 23w14a).
fn quick_play(version_id: &str) -> bool {
    let mut parts = version_id.split(['.', '-', ' ']);
    match (
        parts.next(),
        parts.next().and_then(|m| m.parse::<u32>().ok()),
    ) {
        (Some("1"), Some(minor)) => minor >= 20,
        // Snapshots such as 24w14a, and versions without the 1.x form.
        _ => !version_id.starts_with("1.") && !version_id.starts_with(['a', 'b', 'c', 'r']),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn addresses_are_names_or_ip_addresses_with_a_port() {
        for (text, host, port) in [
            ("play.example.com:25565", "play.example.com", 25565),
            ("Play.Example.COM", "play.example.com", 25565),
            ("192.0.2.7:25570", "192.0.2.7", 25570),
            ("[2001:db8::1]:25565", "[2001:db8::1]", 25565),
            ("[2001:db8::1]", "[2001:db8::1]", 25565),
            ("mc-1.example.org:1", "mc-1.example.org", 1),
        ] {
            assert_eq!(parse_address(text), Some((host.to_owned(), port)), "{text}");
        }
        for bad in [
            "",
            "play.example.com:0",
            "play.example.com:65536",
            "play.example.com:-1",
            "play example.com",
            "play.example.com; rm -rf /",
            "--server evil",
            "-example.com",
            "example-.com",
            "999.1.1.1",
            "1.2.3",
            "[::1",
            "[not-ipv6]:25565",
            "host:25565:1",
            "http://example.com",
            "a\nb.com",
            &format!("{}.com", "a".repeat(64)),
        ] {
            assert_eq!(parse_address(bad), None, "{bad:?}");
        }
    }

    #[test]
    fn the_server_comes_from_the_games_line() {
        assert_eq!(
            connecting_to("[12:01:02] [Render thread/INFO]: Connecting to Play.Example.com, 25565")
                .as_deref(),
            Some("play.example.com:25565")
        );
        assert_eq!(
            connecting_to(
                "<log4j:Message><![CDATA[Connecting to 192.0.2.7, 25570]]></log4j:Message>"
            )
            .as_deref(),
            Some("192.0.2.7:25570")
        );
        assert_eq!(connecting_to("Connecting to evil host, 1"), None);
        assert_eq!(connecting_to("Connected to the world"), None);
    }

    #[test]
    fn new_versions_join_with_quick_play() {
        assert_eq!(
            join_arguments("1.21.1", "play.example.com", 25565),
            ["--quickPlayMultiplayer", "play.example.com:25565"]
        );
        assert_eq!(
            join_arguments("1.20", "[2001:db8::1]", 25565),
            ["--quickPlayMultiplayer", "[2001:db8::1]:25565"]
        );
        assert_eq!(
            join_arguments("24w14a", "a.example.com", 25565)[0],
            "--quickPlayMultiplayer"
        );
        assert_eq!(
            join_arguments("1.12.2", "play.example.com", 25566),
            ["--server", "play.example.com", "--port", "25566"]
        );
        assert_eq!(join_arguments("b1.7.3", "a.example.com", 1)[0], "--server");
    }
}
