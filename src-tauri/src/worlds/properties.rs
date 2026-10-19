//! Typed `server.properties` for a room server (ADAPTATION A2, A3).
//!
//! The file is written in full before every start, so nothing a previous run or
//! a person left in it survives: offline mode, the whitelist, the loopback bind
//! and the per-run RCON password are fixed here, and the only free text, the
//! seed, is validated and escaped.

use serde::{Deserialize, Serialize};

use crate::error::{LauncherError, LauncherResult};
use crate::secrets::SecretString;

pub const MAX_SEED_CHARS: usize = 64;
pub const MAX_PLAYERS: u8 = 20;
pub const DEFAULT_MAX_PLAYERS: u8 = 8;

#[derive(Debug, Clone, Copy, PartialEq, Eq, Default, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum Difficulty {
    Peaceful,
    Easy,
    #[default]
    Normal,
    Hard,
}

impl Difficulty {
    fn as_str(self) -> &'static str {
        match self {
            Self::Peaceful => "peaceful",
            Self::Easy => "easy",
            Self::Normal => "normal",
            Self::Hard => "hard",
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Default, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum GameMode {
    #[default]
    Survival,
    Creative,
    Adventure,
}

impl GameMode {
    fn as_str(self) -> &'static str {
        match self {
            Self::Survival => "survival",
            Self::Creative => "creative",
            Self::Adventure => "adventure",
        }
    }
}

/// The world preset, `level-type` in the file.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum Preset {
    #[default]
    Normal,
    Flat,
    LargeBiomes,
    Amplified,
}

impl Preset {
    fn as_str(self) -> &'static str {
        match self {
            Self::Normal => "minecraft:normal",
            Self::Flat => "minecraft:flat",
            Self::LargeBiomes => "minecraft:large_biomes",
            Self::Amplified => "minecraft:amplified",
        }
    }
}

/// What the player chooses for a world.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct WorldSettings {
    /// Empty for a random seed.
    #[serde(default)]
    pub seed: String,
    #[serde(default)]
    pub preset: Preset,
    #[serde(default)]
    pub difficulty: Difficulty,
    #[serde(default)]
    pub game_mode: GameMode,
    #[serde(default)]
    pub hardcore: bool,
    #[serde(default = "yes")]
    pub pvp: bool,
    #[serde(default = "default_max_players")]
    pub max_players: u8,
}

fn yes() -> bool {
    true
}

fn default_max_players() -> u8 {
    DEFAULT_MAX_PLAYERS
}

impl Default for WorldSettings {
    fn default() -> Self {
        Self {
            seed: String::new(),
            preset: Preset::default(),
            difficulty: Difficulty::default(),
            game_mode: GameMode::default(),
            hardcore: false,
            pvp: true,
            max_players: DEFAULT_MAX_PLAYERS,
        }
    }
}

impl WorldSettings {
    pub fn validate(&self) -> LauncherResult<()> {
        if self.seed.chars().count() > MAX_SEED_CHARS
            || self.seed.chars().any(char::is_control)
            || self.seed.trim() != self.seed
        {
            return Err(LauncherError::invalid_input(
                "A seed has at most 64 characters, without line breaks or surrounding spaces.",
            ));
        }
        if !(1..=MAX_PLAYERS).contains(&self.max_players) {
            return Err(LauncherError::invalid_input(
                "A world allows 1 to 20 players.",
            ));
        }
        Ok(())
    }
}

/// Where one run listens; chosen anew for every start.
pub struct RunBinding {
    pub port: u16,
    pub rcon_port: u16,
    pub rcon_password: SecretString,
}

/// The whole file. `settings` must have passed [`WorldSettings::validate`].
pub fn render(settings: &WorldSettings, binding: &RunBinding) -> String {
    let difficulty = if settings.hardcore {
        Difficulty::Hard
    } else {
        settings.difficulty
    };
    let entries: [(&str, String); 36] = [
        // Who may enter is decided by the tunnel and the whitelist (A3).
        ("online-mode", "false".into()),
        ("enforce-secure-profile", "false".into()),
        ("white-list", "true".into()),
        ("enforce-whitelist", "true".into()),
        ("prevent-proxy-connections", "false".into()),
        ("accepts-transfers", "false".into()),
        // Only the host's bridge connects, over loopback.
        ("server-ip", "127.0.0.1".into()),
        ("server-port", binding.port.to_string()),
        ("enable-status", "true".into()),
        ("hide-online-players", "false".into()),
        ("enable-query", "false".into()),
        ("enable-rcon", "true".into()),
        ("rcon.port", binding.rcon_port.to_string()),
        ("rcon.password", binding.rcon_password.expose().to_owned()),
        ("broadcast-rcon-to-ops", "false".into()),
        ("broadcast-console-to-ops", "false".into()),
        ("enable-jmx-monitoring", "false".into()),
        // Every address is 127.0.0.1; the bridge matches the logged local port
        // to a player for slot accounting and kicks (A3).
        ("log-ips", "true".into()),
        ("motd", "Minecraft World".into()),
        // The world.
        ("level-name", "world".into()),
        ("level-seed", escape(&settings.seed)),
        ("level-type", escape(settings.preset.as_str())),
        ("generate-structures", "true".into()),
        ("difficulty", difficulty.as_str().into()),
        ("gamemode", settings.game_mode.as_str().into()),
        ("force-gamemode", "false".into()),
        ("hardcore", settings.hardcore.to_string()),
        ("pvp", settings.pvp.to_string()),
        ("max-players", settings.max_players.to_string()),
        ("spawn-protection", "0".into()),
        ("allow-flight", "false".into()),
        ("enable-command-block", "false".into()),
        ("op-permission-level", "4".into()),
        ("function-permission-level", "2".into()),
        ("sync-chunk-writes", "true".into()),
        ("player-idle-timeout", "0".into()),
    ];
    let mut text = String::from("# Written by the launcher before every start.\n");
    for (key, value) in entries {
        text.push_str(key);
        text.push('=');
        text.push_str(&value);
        text.push('\n');
    }
    text
}

/// Escapes a value as `java.util.Properties` reads it; everything outside
/// printable ASCII becomes `\uXXXX`, so the file reads the same as UTF-8 or
/// ISO-8859-1.
pub fn escape(value: &str) -> String {
    let mut out = String::with_capacity(value.len());
    for (index, c) in value.chars().enumerate() {
        match c {
            '\\' => out.push_str("\\\\"),
            '\n' => out.push_str("\\n"),
            '\r' => out.push_str("\\r"),
            '\t' => out.push_str("\\t"),
            '\u{c}' => out.push_str("\\f"),
            '=' | ':' | '#' | '!' => {
                out.push('\\');
                out.push(c);
            }
            ' ' if index == 0 => out.push_str("\\ "),
            ' '..='~' => out.push(c),
            _ => {
                let mut units = [0u16; 2];
                for unit in c.encode_utf16(&mut units) {
                    out.push_str(&format!("\\u{unit:04x}"));
                }
            }
        }
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;

    fn binding() -> RunBinding {
        RunBinding {
            port: 40001,
            rcon_port: 40002,
            rcon_password: SecretString::new("0123abcd"),
        }
    }

    /// Reads the file back the way `java.util.Properties` does (the subset the
    /// escaper produces), so injection attempts are checked end to end.
    fn parse(text: &str) -> Vec<(String, String)> {
        let mut out = Vec::new();
        for line in text.lines() {
            if line.starts_with('#') || line.is_empty() {
                continue;
            }
            let mut key = String::new();
            let mut value = String::new();
            let mut in_value = false;
            let mut chars = line.chars();
            while let Some(c) = chars.next() {
                let target = if in_value { &mut value } else { &mut key };
                match c {
                    '\\' => match chars.next() {
                        Some('n') => target.push('\n'),
                        Some('r') => target.push('\r'),
                        Some('t') => target.push('\t'),
                        Some('f') => target.push('\u{c}'),
                        Some('u') => {
                            let hex: String = chars.by_ref().take(4).collect();
                            let unit = u16::from_str_radix(&hex, 16).unwrap();
                            if (0xD800..0xDC00).contains(&unit) {
                                // A surrogate pair is written as two escapes.
                                let low: String = chars.by_ref().skip(2).take(4).collect();
                                let low = u16::from_str_radix(&low, 16).unwrap();
                                target.push_str(&String::from_utf16(&[unit, low]).unwrap());
                            } else {
                                target.push_str(&String::from_utf16(&[unit]).unwrap());
                            }
                        }
                        Some(other) => target.push(other),
                        None => {}
                    },
                    '=' if !in_value => in_value = true,
                    other => target.push(other),
                }
            }
            out.push((key, value));
        }
        out
    }

    fn value(text: &str, key: &str) -> Vec<String> {
        parse(text)
            .into_iter()
            .filter(|(k, _)| k == key)
            .map(|(_, v)| v)
            .collect()
    }

    #[test]
    fn fixed_security_settings() {
        let text = render(&WorldSettings::default(), &binding());
        for (key, expected) in [
            ("online-mode", "false"),
            ("white-list", "true"),
            ("enforce-whitelist", "true"),
            ("server-ip", "127.0.0.1"),
            ("server-port", "40001"),
            ("enable-query", "false"),
            ("enable-rcon", "true"),
            ("rcon.port", "40002"),
            ("rcon.password", "0123abcd"),
            ("accepts-transfers", "false"),
            ("level-type", "minecraft:normal"),
        ] {
            assert_eq!(value(&text, key), vec![expected.to_owned()], "{key}");
        }
        let keys: Vec<_> = parse(&text).into_iter().map(|(k, _)| k).collect();
        let mut unique = keys.clone();
        unique.sort();
        unique.dedup();
        assert_eq!(keys.len(), unique.len(), "every key once");
    }

    #[test]
    fn a_seed_cannot_add_or_override_keys() {
        for seed in [
            "1\nonline-mode=true",
            "x\\\nwhite-list=false",
            "=online-mode",
            "a:b#c!d",
            "\u{2028}server-ip=0.0.0.0",
            "семя 🌱",
        ] {
            let settings = WorldSettings {
                seed: seed.to_owned(),
                ..WorldSettings::default()
            };
            let text = render(&settings, &binding());
            assert_eq!(
                value(&text, "level-seed"),
                vec![seed.to_owned()],
                "{seed:?}"
            );
            assert_eq!(value(&text, "online-mode"), vec!["false".to_owned()]);
            assert_eq!(value(&text, "white-list"), vec!["true".to_owned()]);
            assert_eq!(value(&text, "server-ip"), vec!["127.0.0.1".to_owned()]);
            assert!(text.is_ascii());
        }
    }

    #[test]
    fn settings_are_bounded() {
        let mut settings = WorldSettings::default();
        assert!(settings.validate().is_ok());
        for seed in ["a\nb", " lead", "trail ", &"9".repeat(65)] {
            settings.seed = seed.to_owned();
            assert!(settings.validate().is_err(), "{seed:?}");
        }
        settings.seed = "-1234567890".into();
        assert!(settings.validate().is_ok());
        for players in [0, 21, 255] {
            settings.max_players = players;
            assert!(settings.validate().is_err());
        }
    }

    #[test]
    fn hardcore_is_always_hard() {
        let settings = WorldSettings {
            hardcore: true,
            difficulty: Difficulty::Peaceful,
            ..WorldSettings::default()
        };
        let text = render(&settings, &binding());
        assert_eq!(value(&text, "difficulty"), vec!["hard".to_owned()]);
        assert_eq!(value(&text, "hardcore"), vec!["true".to_owned()]);
    }

    #[test]
    fn settings_reject_unknown_fields() {
        let json = r#"{"seed":"","onlineMode":true}"#;
        assert!(serde_json::from_str::<WorldSettings>(json).is_err());
        let json = r#"{"maxPlayers":4}"#;
        let parsed: WorldSettings = serde_json::from_str(json).unwrap();
        assert_eq!(parsed.max_players, 4);
        assert!(parsed.pvp);
    }
}
