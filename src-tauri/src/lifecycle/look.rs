//! How the main window looks before its page paints. WebView2 takes about
//! 0.4 s to start, and the window stood white meanwhile, a flash in the dark
//! theme. The window and WebView2 now take the interface's ground in the theme
//! the page will show, and a script run before the page's own marks the
//! document so index.html's placeholder of the shell shows in that theme and
//! direction until the interface draws.

use serde::Deserialize;
use tauri::window::Color;

use crate::storage::models::Theme;

/// The interface's ground, `--color-bg` in src/styles/tokens.css.
pub const LIGHT_GROUND: Color = Color(0xf3, 0xf5, 0xf0, 0xff);
pub const DARK_GROUND: Color = Color(0x0f, 0x17, 0x13, 0xff);

/// The interface's languages with their direction (written by scripts/i18n.mjs).
const LANGUAGES: &str = include_str!("../../../src/locales/languages.json");

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct FirstLook {
    /// None when the theme follows a system whose choice is not known here.
    pub ground: Option<Color>,
    /// Sets `data-boot`, the chosen theme and the direction on the document
    /// of the window's top frame. Only fixed words go into it.
    pub script: String,
}

/// The look for `theme` and the interface `language`; `system_dark` is the
/// system's choice where it can be read.
pub fn first_look(theme: Theme, language: &str, system_dark: Option<bool>) -> FirstLook {
    let ground = |dark: bool| if dark { DARK_GROUND } else { LIGHT_GROUND };
    let (ground, chosen) = match theme {
        Theme::Light => (Some(LIGHT_GROUND), Some("r.dataset.theme = 'light'")),
        Theme::Dark => (Some(DARK_GROUND), Some("r.dataset.theme = 'dark'")),
        // The page follows the system through prefers-color-scheme.
        Theme::System => (system_dark.map(ground), None),
    };
    let mut set = vec!["r.dataset.boot = ''"];
    set.extend(chosen);
    if right_to_left(language) {
        set.push("r.dir = 'rtl'");
    }
    // The script runs as the document is created, before its <html> element:
    // the marks go on as soon as the parser adds it, before the first paint.
    let script = format!(
        "(function () {{ if (window.top !== window) return; \
         function mark(r) {{ {}; }} \
         if (document.documentElement) {{ mark(document.documentElement); return; }} \
         var o = new MutationObserver(function () {{ \
         var r = document.documentElement; if (r) {{ o.disconnect(); mark(r); }} }}); \
         o.observe(document, {{ childList: true }}); }})();",
        set.join("; ")
    );
    FirstLook { ground, script }
}

/// Whether the interface shows `language` right to left: the tag itself, else
/// its base language, else English, as src/lib/strings.ts chooses.
fn right_to_left(language: &str) -> bool {
    #[derive(Deserialize)]
    struct Listed {
        tag: String,
        dir: String,
    }
    let Ok(listed) = serde_json::from_str::<Vec<Listed>>(LANGUAGES) else {
        return false;
    };
    let base = language.split('-').next().unwrap_or(language);
    [language, base]
        .iter()
        .find_map(|tag| listed.iter().find(|l| l.tag == *tag))
        .is_some_and(|l| l.dir == "rtl")
}

/// Whether Windows shows apps dark (Settings → Personalization → Colors), the
/// choice WebView2's prefers-color-scheme follows.
#[cfg(windows)]
pub fn system_dark() -> Option<bool> {
    windows_registry::CURRENT_USER
        .open(r"Software\Microsoft\Windows\CurrentVersion\Themes\Personalize")
        .and_then(|key| key.get_u32("AppsUseLightTheme"))
        .ok()
        .map(|light| light == 0)
}

/// Elsewhere the system's choice is not read: the window keeps its own ground
/// and the page follows the system as before.
#[cfg(not(windows))]
pub fn system_dark() -> Option<bool> {
    None
}

#[cfg(test)]
mod tests {
    use super::*;

    fn hex(color: Color) -> String {
        format!("#{:02x}{:02x}{:02x}", color.0, color.1, color.2)
    }

    #[test]
    fn the_grounds_are_the_interface_tokens() {
        let tokens = include_str!("../../../src/styles/tokens.css");
        let grounds: Vec<&str> = tokens
            .lines()
            .filter_map(|l| l.trim().strip_prefix("--color-bg:"))
            .map(|v| v.trim().trim_end_matches(';'))
            .collect();
        assert!(grounds.contains(&hex(LIGHT_GROUND).as_str()), "{grounds:?}");
        assert!(grounds.contains(&hex(DARK_GROUND).as_str()), "{grounds:?}");
        assert!(
            grounds
                .iter()
                .all(|g| *g == hex(LIGHT_GROUND) || *g == hex(DARK_GROUND)),
            "{grounds:?}"
        );
    }

    #[test]
    fn the_theme_and_direction_come_from_the_settings() {
        let light = first_look(Theme::Light, "en", Some(true));
        assert_eq!(light.ground, Some(LIGHT_GROUND));
        assert!(light.script.contains("r.dataset.theme = 'light'"));
        assert!(!light.script.contains("rtl"));

        let dark = first_look(Theme::Dark, "ar", None);
        assert_eq!(dark.ground, Some(DARK_GROUND));
        assert!(dark.script.contains("r.dataset.theme = 'dark'"));
        assert!(dark.script.contains("r.dir = 'rtl'"));

        // The system's theme: the page follows it on its own.
        let system = first_look(Theme::System, "ru", Some(true));
        assert_eq!(system.ground, Some(DARK_GROUND));
        assert!(!system.script.contains("dataset.theme"));
        assert_eq!(
            first_look(Theme::System, "ru", Some(false)).ground,
            Some(LIGHT_GROUND)
        );
        assert_eq!(first_look(Theme::System, "en", None).ground, None);

        for look in [light, dark, system] {
            assert!(look.script.contains("r.dataset.boot = ''"));
            assert!(look.script.contains("window.top !== window"));
        }
    }

    #[test]
    fn a_region_or_unknown_language_finds_its_direction() {
        assert!(right_to_left("ar"));
        assert!(right_to_left("ar-EG"));
        assert!(!right_to_left("ru"));
        assert!(!right_to_left("pt-BR"));
        assert!(!right_to_left("zz"));
    }
}
