//! What a report must not carry (D-061), removed before the player sees the
//! preview: chat, player names, UUIDs, IP and server addresses, world names
//! and the home folder, on top of the export's token and path redaction.
//! Mod names and versions stay; without them a report is useless.

use std::path::Path;
use std::sync::LazyLock;

use regex::Regex;
use serde::Serialize;

/// Who and where to hide.
pub struct Context<'a> {
    /// Player names of saved accounts and no-token profiles.
    pub names: Vec<String>,
    pub home: Option<&'a Path>,
    pub data_root: &'a Path,
}

/// How much was replaced, shown with the preview.
#[derive(Debug, Default, Clone, Copy, PartialEq, Eq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct Counts {
    pub chat_lines: usize,
    pub names: usize,
    pub addresses: usize,
    pub worlds: usize,
    pub ids: usize,
}

impl Counts {
    pub fn add(&mut self, other: Counts) {
        self.chat_lines += other.chat_lines;
        self.names += other.names;
        self.addresses += other.addresses;
        self.worlds += other.worlds;
        self.ids += other.ids;
    }
}

fn re(pattern: &str) -> Regex {
    Regex::new(pattern).expect("scrub pattern")
}

/// `[CHAT] …` lines and the `<Name> text` chat of older versions.
static CHAT: LazyLock<Regex> =
    LazyLock::new(|| re(r"\[CHAT\]|\]: <[A-Za-z0-9_]{1,16}> |^<[A-Za-z0-9_]{1,16}> "));
/// The player name the game prints itself.
static PLAYER: LazyLock<Regex> = LazyLock::new(|| {
    re(r"(Setting user: |--username[ =,]+|\buser(?:name)?[=:] ?|Player name: )([A-Za-z0-9_]{1,16})")
});
static UUID: LazyLock<Regex> = LazyLock::new(|| {
    re(r"(?i)\b[0-9a-f]{8}-?[0-9a-f]{4}-?[0-9a-f]{4}-?[0-9a-f]{4}-?[0-9a-f]{12}\b")
});
static IPV4: LazyLock<Regex> =
    LazyLock::new(|| re(r"(^|[^\w.])((?:\d{1,3}\.){3}\d{1,3})(:\d{1,5})?($|[^\w.])"));
/// Candidates only; [`std::net::Ipv6Addr`] decides, so `12:00:03` is a time.
static IPV6: LazyLock<Regex> =
    LazyLock::new(|| re(r"(?i)\[?[0-9a-f]{0,4}:[0-9a-f:]*:[0-9a-f]{0,4}\]?"));
/// "Connecting to play.example.net, 25565", `--server`, quick play.
static SERVER: LazyLock<Regex> = LazyLock::new(|| {
    re(
        r"(Connecting to |--server[ =,]+|--quickPlayMultiplayer[ =,]+|Joining server |Server address: )([^\s,]+)",
    )
});
static WORLD_PATH: LazyLock<Regex> = LazyLock::new(|| re(r"(saves[/\\])([^/\\\r\n]+)"));
static WORLD_NAME: LazyLock<Regex> = LazyLock::new(|| {
    re(r#"(Preparing level "|ServerLevel\[|--quickPlaySingleplayer[ =,]+|level ')([^"\]']+)"#)
});

fn valid_ipv4(text: &str) -> bool {
    let parts: Vec<&str> = text.split('.').collect();
    parts.len() == 4
        && parts.iter().all(|p| p.parse::<u8>().is_ok())
        && !matches!(text, "0.0.0.0" | "127.0.0.1")
}

fn replace_words(line: &str, name: &str, counts: &mut usize) -> String {
    if name.len() < 3 || !line.contains(name) {
        return line.to_owned();
    }
    let pattern = format!(r"(^|[^A-Za-z0-9_]){}($|[^A-Za-z0-9_])", regex::escape(name));
    let Ok(word) = Regex::new(&pattern) else {
        return line.to_owned();
    };
    let replaced = word.replace_all(line, "${1}<player>${2}");
    if replaced != line {
        *counts += 1;
    }
    replaced.into_owned()
}

/// One line, already free of tokens and private paths.
fn line(text: &str, cx: &Context<'_>, counts: &mut Counts) -> String {
    if CHAT.is_match(text) {
        counts.chat_lines += 1;
        return "[chat line removed]".to_owned();
    }
    let mut out = text.to_owned();
    if PLAYER.is_match(&out) {
        counts.names += 1;
        out = PLAYER.replace_all(&out, "${1}<player>").into_owned();
    }
    for name in &cx.names {
        out = replace_words(&out, name, &mut counts.names);
    }
    if SERVER.is_match(&out) {
        counts.addresses += 1;
        out = SERVER.replace_all(&out, "${1}<server>").into_owned();
    }
    let ips = IPV4
        .replace_all(&out, |c: &regex::Captures<'_>| {
            if valid_ipv4(&c[2]) {
                counts.addresses += 1;
                format!("{}<ip>{}", &c[1], &c[4])
            } else {
                c[0].to_owned()
            }
        })
        .into_owned();
    out = ips;
    let ips = IPV6
        .replace_all(&out, |c: &regex::Captures<'_>| {
            let bare = c[0].trim_start_matches('[').trim_end_matches(']');
            match bare.parse::<std::net::Ipv6Addr>() {
                Ok(ip) if !ip.is_loopback() && !ip.is_unspecified() => {
                    counts.addresses += 1;
                    "<ip>".to_owned()
                }
                _ => c[0].to_owned(),
            }
        })
        .into_owned();
    out = ips;
    if WORLD_PATH.is_match(&out) || WORLD_NAME.is_match(&out) {
        counts.worlds += 1;
        out = WORLD_PATH.replace_all(&out, "${1}<world>").into_owned();
        out = WORLD_NAME.replace_all(&out, "${1}<world>").into_owned();
    }
    if UUID.is_match(&out) {
        counts.ids += 1;
        out = UUID.replace_all(&out, "<uuid>").into_owned();
    }
    out
}

/// A whole text: the export's redaction, then [`line`] per line.
pub fn text(text: &str, cx: &Context<'_>) -> (String, Counts) {
    let sanitized = crate::diagnostics::sanitize(text, cx.home, cx.data_root);
    let mut counts = Counts::default();
    let lines: Vec<String> = sanitized
        .lines()
        .map(|l| line(l, cx, &mut counts))
        .collect();
    let mut out = lines.join("\n");
    if sanitized.ends_with('\n') {
        out.push('\n');
    }
    (out, counts)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn cx(root: &Path) -> Context<'_> {
        Context {
            names: vec!["Alex_Tester".into(), "Bo".into()],
            home: Some(Path::new("/home/jurgen")),
            data_root: root,
        }
    }

    #[test]
    fn private_things_are_removed_and_mods_stay() {
        let root = Path::new("/home/jurgen/.local/share/app.launcher");
        let input = "\
[12:00:01] [Render thread/INFO]: Setting user: Alex_Tester
[12:00:02] [Render thread/INFO]: [System] [CHAT] Alex_Tester joined the game
[12:00:02] [Server thread/INFO]: <Alex_Tester> my password is hunter2
[12:00:03] [Render thread/INFO]: Connecting to play.example.net, 25565
[12:00:03] [Netty/INFO]: Connected to 203.0.113.9:25565 and [2001:db8::1:2]
[12:00:04] [Server thread/INFO]: Preparing level \"My Secret Base\"
[12:00:04] [Server thread/INFO]: Saving chunks for level 'ServerLevel[My Secret Base]'/minecraft:overworld
Loading /home/jurgen/.local/share/app.launcher/instances/x/game/saves/My Secret Base/level.dat
UUID of player Alex_Tester is 069a79f4-44e9-4726-a5be-fca90e38aaf5
Mod 'GeckoLib 4' (geckolib) 4.8.4 requires version 0.92.0+1.20.1 of fabric-api
Forge 1.12.2-14.23.5.2859, Java 21.0.7, OpenGL 4.6.0 NVIDIA 555.42.02
Bob says hi to Bo
";
        let (out, counts) = text(input, &cx(root));
        for secret in [
            "Alex_Tester",
            "hunter2",
            "play.example.net",
            "203.0.113.9",
            "2001:db8",
            "My Secret Base",
            "069a79f4",
            "jurgen",
        ] {
            assert!(!out.contains(secret), "{secret} in:\n{out}");
        }
        assert!(out.contains(
            "Mod 'GeckoLib 4' (geckolib) 4.8.4 requires version 0.92.0+1.20.1 of fabric-api"
        ));
        assert!(
            out.contains("14.23.5.2859"),
            "versions are not addresses:\n{out}"
        );
        assert!(out.contains("555.42.02"));
        assert!(out.contains("Bob says hi"), "names match whole words only");
        assert_eq!(counts.chat_lines, 2);
        assert!(
            counts.addresses >= 3 && counts.worlds >= 2 && counts.ids == 1,
            "{counts:?}"
        );
        assert!(out.ends_with('\n'));
    }
}
