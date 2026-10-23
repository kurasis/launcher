//! Forge's `config/forge.cfg` on a world server before 1.13, which keeps the
//! update check off.
//!
//! Forge 1.7.10 to 1.12.2 starts its update check in a thread of its own while
//! the mods load. Under Java 8 that thread can deadlock with the server's main
//! thread: one initialises `NetworkInterface` and waits for the lock of
//! `System.loadLibrary`, the other holds it while loading Netty's epoll library,
//! whose `NetUtil` needs `NetworkInterface`. The start then hangs after
//! "Starting Minecraft server on". With `general { B:disableVersionCheck=true }`
//! the thread never starts, and the check is of no use on a server anyway.
//!
//! Forge adds its other settings to the file itself. A file from the world's
//! mods keeps what it says: the setting is added after it, and the last value
//! of a property is the one Forge reads.

use std::path::Path;

use crate::storage::atomic;

/// Where Forge reads its settings, under the server directory.
pub const PATH: &str = "config/forge.cfg";
/// The most of an existing file read; Forge's own is a few KiB.
const MAX_BYTES: u64 = 256 * 1024;
/// What is written, or added after a file that does not turn the check off.
const TURNED_OFF: &str = "# The launcher turns the update check off: under Java 8 it can hang the start.\n\
general {\n    B:disableVersionCheck=true\n}\n";

/// Makes `config/forge.cfg` in `server_dir` turn Forge's update check off. A
/// file that already does is left as it is; one that cannot be read in Forge's
/// format, or is larger than [`MAX_BYTES`], is replaced.
pub fn turn_off_update_check(server_dir: &Path) -> std::io::Result<()> {
    let path = server_dir.join(PATH);
    let kept = std::fs::symlink_metadata(&path)
        .ok()
        .filter(|meta| meta.is_file() && meta.len() <= MAX_BYTES)
        .and_then(|_| std::fs::read_to_string(&path).ok());
    let text = match kept.as_deref().map(read) {
        Some(Some(true)) => return Ok(()),
        Some(Some(false)) => {
            let mut text = kept.unwrap_or_default();
            if !text.is_empty() && !text.ends_with('\n') {
                text.push('\n');
            }
            text + TURNED_OFF
        }
        _ => TURNED_OFF.to_owned(),
    };
    if let Some(config) = path.parent() {
        std::fs::create_dir_all(config)?;
    }
    atomic::write(&path, text.as_bytes())
}

/// Whether a `forge.cfg` turns the update check off, as Forge reads it: the
/// last `disableVersionCheck` of the top-level `general` category. `None` when
/// its categories or lists do not close.
fn read(text: &str) -> Option<bool> {
    let mut categories: Vec<String> = Vec::new();
    let mut in_list = false;
    let mut off = false;
    for line in text.lines().map(str::trim) {
        if in_list {
            in_list = line != ">";
            continue;
        }
        if line.is_empty() || line.starts_with('#') {
            continue;
        }
        if line == "}" {
            categories.pop()?;
        } else if let Some(name) = line.strip_suffix('{') {
            categories.push(unquote(name.trim()));
        } else if line.ends_with('<') {
            in_list = true;
        } else if let Some((name, value)) = line.split_once('=')
            && categories.len() == 1
            && categories[0] == "general"
            && unquote(name.trim()) == "B:disableVersionCheck"
        {
            off = value.trim().eq_ignore_ascii_case("true");
        }
    }
    (categories.is_empty() && !in_list).then_some(off)
}

/// A name without Forge's quotes: `"general"`, or `B:"disableVersionCheck"`.
fn unquote(name: &str) -> String {
    name.replace('"', "")
}

#[cfg(test)]
mod tests {
    use super::*;

    fn written(existing: Option<&[u8]>) -> String {
        let dir = tempfile::tempdir().unwrap();
        if let Some(bytes) = existing {
            std::fs::create_dir_all(dir.path().join("config")).unwrap();
            std::fs::write(dir.path().join(PATH), bytes).unwrap();
        }
        turn_off_update_check(dir.path()).unwrap();
        let text = std::fs::read_to_string(dir.path().join(PATH)).unwrap();
        assert_eq!(read(&text), Some(true), "{text}");
        text
    }

    #[test]
    fn the_update_check_is_turned_off() {
        assert_eq!(written(None), TURNED_OFF);

        // As Forge 1.12.2 saves it, with the check on.
        let forge = "# Configuration file\n\ngeneral {\n    \
            # Set to true to disable Forge's version check mechanics.\n    \
            B:disableVersionCheck=false\n    I:clumpingThreshold=64\n    \
            S:\"some list\" <\n        a {\n        }\n     >\n}\n\n\
            version_checking {\n    B:Global=true\n}\n";
        assert_eq!(
            written(Some(forge.as_bytes())),
            format!("{forge}{TURNED_OFF}")
        );
        // Without a newline at the end, or without the property.
        let short = "general {\n    I:clumpingThreshold=64\n}";
        assert_eq!(
            written(Some(short.as_bytes())),
            format!("{short}\n{TURNED_OFF}")
        );

        // Already off: left as it is, so no start makes the file grow.
        let off = forge.replace("disableVersionCheck=false", "disableVersionCheck=TRUE");
        assert_eq!(written(Some(off.as_bytes())), off);
        let quoted = "\"general\" {\n    B:\"disableVersionCheck\"=true\n}\n";
        assert_eq!(written(Some(quoted.as_bytes())), quoted);
        assert_eq!(
            written(Some(written(Some(short.as_bytes())).as_bytes()))
                .matches("general {")
                .count(),
            2
        );
    }

    #[test]
    fn only_the_general_category_counts() {
        for text in [
            "client {\n    B:disableVersionCheck=true\n}\n",
            "general {\n    client {\n        B:disableVersionCheck=true\n    }\n}\n",
            "general {\n    S:disableVersionCheck <\n        B:disableVersionCheck=true\n     >\n}\n",
            "general {\n    # B:disableVersionCheck=true\n    B:disableVersionCheck=yes\n}\n",
            "general {\n    B:disableVersionCheck=true\n    B:disableVersionCheck=false\n}\n",
        ] {
            assert_eq!(read(text), Some(false), "{text}");
            assert_eq!(
                written(Some(text.as_bytes())),
                format!("{text}{TURNED_OFF}")
            );
        }
    }

    #[test]
    fn a_file_forge_cannot_read_is_replaced() {
        for junk in [
            &b"general {\n    B:disableVersionCheck=true\n"[..],
            b"}\ngeneral {\n}\n",
            b"general {\n    S:list <\n        a\n",
            b"\xff\xfe\x00not text",
            &vec![b'#'; usize::try_from(MAX_BYTES).unwrap() + 1],
        ] {
            assert_eq!(written(Some(junk)), TURNED_OFF);
        }
        // A directory in its place is not written through.
        let dir = tempfile::tempdir().unwrap();
        std::fs::create_dir_all(dir.path().join(PATH)).unwrap();
        assert!(turn_off_update_check(dir.path()).is_err());
    }
}
