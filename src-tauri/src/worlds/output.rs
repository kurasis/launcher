//! Reading the server's console output (PLAN W3, ADAPTATION A3).
//!
//! Lines are classified as they arrive and then dropped: the launcher keeps no
//! server log of its own, so chat and nicks are not stored by default. Only
//! the server thread's own messages count; a chat line starts with `<name>`,
//! `/say` with `[name]` and `/me` with `* name`, so a player cannot type a line
//! that is read as an event.

use super::identity::Nick;

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Event {
    /// `Done (1.234s)! For help, type "help"`.
    Ready,
    /// The port was taken between choosing and binding it.
    BindFailed,
    /// The server refused to start without `eula=true`.
    EulaRequired,
    /// `Name[/127.0.0.1:port] logged in with entity id …`.
    Joined { nick: Nick, local_port: u16 },
    /// `Name lost connection: …`.
    Left { nick: Nick },
}

/// The message of a line from the server's main thread, after its prefix
/// `[hh:mm:ss] [Server thread/LEVEL]: `.
fn server_message(line: &str) -> Option<&str> {
    let rest = line.strip_prefix('[')?;
    let (_, rest) = rest.split_once("] [")?;
    let (thread, message) = rest.split_once("]: ")?;
    let (thread, _level) = thread.rsplit_once('/')?;
    (thread == "Server thread" || thread == "main").then_some(message)
}

pub fn classify(line: &str) -> Option<Event> {
    let message = server_message(line)?;
    if message.starts_with("Done (") && message.ends_with(r#"For help, type "help""#) {
        return Some(Event::Ready);
    }
    if message.starts_with("**** FAILED TO BIND TO PORT!") {
        return Some(Event::BindFailed);
    }
    if message.starts_with("You need to agree to the EULA") {
        return Some(Event::EulaRequired);
    }
    if let Some((name, rest)) = message.split_once("[/127.0.0.1:")
        && let Some((port, tail)) = rest.split_once("] logged in with entity id ")
        && tail.bytes().next().is_some_and(|b| b.is_ascii_digit())
    {
        let nick = Nick::parse(name).ok()?;
        let local_port = port.parse().ok()?;
        return Some(Event::Joined { nick, local_port });
    }
    if let Some((name, _)) = message.split_once(" lost connection: ") {
        return Some(Event::Left {
            nick: Nick::parse(name).ok()?,
        });
    }
    None
}

#[cfg(test)]
mod tests {
    use super::*;

    fn nick(name: &str) -> Nick {
        Nick::parse(name).unwrap()
    }

    #[test]
    fn server_events() {
        assert_eq!(
            classify(r#"[19:20:01] [Server thread/INFO]: Done (4.512s)! For help, type "help""#),
            Some(Event::Ready)
        );
        assert_eq!(
            classify("[19:20:01] [Server thread/WARN]: **** FAILED TO BIND TO PORT!"),
            Some(Event::BindFailed)
        );
        assert_eq!(
            classify(
                "[19:20:01] [main/INFO]: You need to agree to the EULA in order to run the server. Go to eula.txt for more info."
            ),
            Some(Event::EulaRequired)
        );
        assert_eq!(
            classify(
                "[19:21:00] [Server thread/INFO]: Notch[/127.0.0.1:52344] logged in with entity id 112 at (8.5, 64.0, 8.5)"
            ),
            Some(Event::Joined {
                nick: nick("Notch"),
                local_port: 52344
            })
        );
        assert_eq!(
            classify("[19:25:00] [Server thread/INFO]: Notch lost connection: Disconnected"),
            Some(Event::Left {
                nick: nick("Notch")
            })
        );
    }

    #[test]
    fn players_cannot_forge_events() {
        for line in [
            r#"[19:21:00] [Server thread/INFO]: <Mallory> Done (1s)! For help, type "help""#,
            "[19:21:00] [Server thread/INFO]: <Mallory> Notch[/127.0.0.1:1] logged in with entity id 1 at (0, 0, 0)",
            "[19:21:00] [Server thread/INFO]: [Mallory] Notch lost connection: x",
            "[19:21:00] [Server thread/INFO]: * Mallory Notch lost connection: x",
            "[19:21:00] [User Authenticator #1/INFO]: Notch[/127.0.0.1:1] logged in with entity id 1",
            "[19:21:00] [Server thread/INFO]: Notch[/10.0.0.1:1] logged in with entity id 1",
            "[19:21:00] [Server thread/INFO]: Notch[/127.0.0.1:99999] logged in with entity id 1",
            "Notch lost connection: Disconnected",
            "",
        ] {
            assert_eq!(classify(line), None, "{line}");
        }
    }
}
