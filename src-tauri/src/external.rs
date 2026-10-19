//! Opening external HTTPS pages in the system browser through a Rust allowlist
//! (docs/spec/07 "Trust boundaries"). The webview never receives a generic opener
//! permission; it can only ask for one of a few fixed destinations.

use serde::{Deserialize, Serialize};
use url::Url;

use crate::error::{ErrorCode, LauncherError, LauncherResult};

/// Fixed official destinations the UI may open.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum ExternalLink {
    BuyMinecraft,
    CreateProfile,
    XboxProfile,
    FamilySettings,
    MinecraftHelp,
    MinecraftEula,
    /// This project's source repository.
    SourceCode,
}

impl ExternalLink {
    pub fn url(self) -> &'static str {
        match self {
            Self::BuyMinecraft => {
                "https://www.minecraft.net/en-us/store/minecraft-java-bedrock-edition-pc"
            }
            Self::CreateProfile => "https://www.minecraft.net/en-us/msaprofile",
            Self::XboxProfile => "https://www.xbox.com/en-US/live",
            Self::FamilySettings => "https://account.microsoft.com/family",
            Self::MinecraftHelp => "https://help.minecraft.net/",
            Self::MinecraftEula => "https://www.minecraft.net/en-us/eula",
            Self::SourceCode => "https://github.com/kurasis/launcher",
        }
    }
}

/// Hosts the launcher may open. The sign-in host is allowed only for the
/// authorization endpoint path. The feed (D-067) links to Minecraft's
/// articles, to YouTube and Twitch, and to its own pages.
const ALLOWED_HOSTS: &[&str] = &[
    "www.minecraft.net",
    "help.minecraft.net",
    "www.xbox.com",
    "account.microsoft.com",
    "feed.lapi.sh",
    "www.youtube.com",
    "youtu.be",
    "www.twitch.tv",
];
const SIGN_IN_HOST: &str = "login.microsoftonline.com";
const SIGN_IN_PATH: &str = "/consumers/oauth2/v2.0/authorize";
/// github.com is allowed only for this project's repository.
const SOURCE_HOST: &str = "github.com";
const SOURCE_PATH: &str = "/kurasis/launcher";

/// Validates a URL against the allowlist: HTTPS, exact host, default port, no
/// credentials.
pub fn check_allowed(raw: &str) -> LauncherResult<Url> {
    let refuse = || {
        LauncherError::new(
            ErrorCode::PermissionDenied,
            "This link is not on the launcher's list of allowed destinations.",
        )
    };
    let url = Url::parse(raw).map_err(|_| refuse())?;
    let host = url.host_str().ok_or_else(refuse)?;
    let source_ok = host == SOURCE_HOST
        && (url.path() == SOURCE_PATH || url.path().starts_with(&format!("{SOURCE_PATH}/")));
    let host_ok = ALLOWED_HOSTS.contains(&host)
        || (host == SIGN_IN_HOST && url.path() == SIGN_IN_PATH)
        || source_ok;
    if url.scheme() != "https"
        || !host_ok
        || url.port().is_some()
        || !url.username().is_empty()
        || url.password().is_some()
    {
        return Err(refuse());
    }
    Ok(url)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn fixed_links_are_allowed() {
        for link in [
            ExternalLink::BuyMinecraft,
            ExternalLink::CreateProfile,
            ExternalLink::XboxProfile,
            ExternalLink::FamilySettings,
            ExternalLink::MinecraftHelp,
            ExternalLink::MinecraftEula,
            ExternalLink::SourceCode,
        ] {
            assert!(check_allowed(link.url()).is_ok(), "{link:?}");
        }
        assert!(
            check_allowed(
                "https://login.microsoftonline.com/consumers/oauth2/v2.0/authorize?client_id=x"
            )
            .is_ok()
        );
    }

    #[test]
    fn feed_destinations_are_allowed() {
        for good in [
            "https://www.youtube.com/watch?v=dQw4w9WgXcQ",
            "https://youtu.be/dQw4w9WgXcQ",
            "https://www.twitch.tv/some_streamer",
            "https://feed.lapi.sh/play/yt/dQw4w9WgXcQ?lang=ru",
            "https://www.minecraft.net/en-us/article/minecraft-java-edition-1-21",
        ] {
            assert!(check_allowed(good).is_ok(), "{good}");
        }
        for bad in [
            "http://www.youtube.com/watch?v=x",
            "https://youtube.com.evil.example/",
            "https://m.youtube.com/watch?v=x",
            "https://twitch.tv.evil.example/",
            "https://www.twitch.tv:444/x",
            "https://user@www.twitch.tv/x",
            "https://evil.lapi.sh/",
            "https://feed.lapi.sh.evil.example/",
            "javascript://www.youtube.com/%0aalert(1)",
            "data:text/html,<script>alert(1)</script>",
        ] {
            assert!(check_allowed(bad).is_err(), "{bad}");
        }
    }

    #[test]
    fn everything_else_is_refused() {
        for bad in [
            "http://www.minecraft.net/",
            "https://www.minecraft.net.evil.example/",
            "https://evil.example/?u=https://www.minecraft.net",
            "https://user:pass@www.minecraft.net/",
            "https://www.minecraft.net:8443/",
            "https://login.microsoftonline.com/common/oauth2/v2.0/logout",
            "file:///etc/passwd",
            "javascript:alert(1)",
            "minecraft://launch",
            "https://github.com/other/repo",
            "https://github.com/kurasis/launcher-evil",
            "",
        ] {
            assert!(check_allowed(bad).is_err(), "{bad}");
        }
    }
}
