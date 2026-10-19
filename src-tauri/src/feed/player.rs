//! Videos and streams of the feed (D-067): in the launcher's player window, or
//! in the system browser.
//!
//! The player window shows feed.lapi.sh's play page, which holds only the
//! provider's embedded player. The window has no capability (its own file
//! grants nothing, and no remote origin is named), so its pages cannot call
//! the launcher. It goes nowhere but the play pages and the embedded players
//! they hold; a link that would open another window opens in the browser
//! when the launcher may open it there, and nothing otherwise.

use tauri::{AppHandle, Manager, Runtime, WebviewUrl, WebviewWindowBuilder};
use url::Url;

use super::model::{Lang, twitch_login, video_id};

/// The player window's label; `capabilities/player.json` grants it nothing.
pub const WINDOW: &str = "player";
const PLAY_HOST: &str = "feed.lapi.sh";

/// What plays.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Target {
    /// A YouTube video by its id.
    Video(String),
    /// A Twitch stream by its channel's login.
    Stream(String),
}

impl Target {
    /// `yt` with a video id, or `twitch` with a login; anything else is `None`.
    pub fn parse(kind: &str, id: &str) -> Option<Self> {
        match kind {
            "yt" if video_id(id) => Some(Self::Video(id.to_owned())),
            "twitch" if twitch_login(id) => Some(Self::Stream(id.to_owned())),
            _ => None,
        }
    }

    /// The play page for the player window.
    pub fn player_url(&self, lang: Lang) -> Url {
        let path = match self {
            Self::Video(id) => format!("yt/{id}"),
            Self::Stream(login) => format!("twitch/{login}"),
        };
        let mut url = Url::parse(&format!("https://{PLAY_HOST}/play/{path}"))
            .unwrap_or_else(|_| unreachable_url());
        url.query_pairs_mut().append_pair("lang", lang.as_str());
        url
    }

    /// The provider's own page, for the browser.
    pub fn browser_url(&self) -> Url {
        let raw = match self {
            Self::Video(id) => format!("https://www.youtube.com/watch?v={id}"),
            Self::Stream(login) => format!("https://www.twitch.tv/{login}"),
        };
        Url::parse(&raw).unwrap_or_else(|_| unreachable_url())
    }
}

/// The ids are checked before an address is made, so this is never used.
fn unreachable_url() -> Url {
    Url::parse("about:blank").expect("about:blank is an address")
}

fn plain_https(url: &Url) -> bool {
    url.scheme() == "https"
        && url.port().is_none()
        && url.username().is_empty()
        && url.password().is_none()
}

/// Where the player window may go: the play pages, and the embedded players
/// they frame (on Linux and macOS a frame's navigation asks here too).
pub fn navigation_allowed(url: &Url) -> bool {
    if url.scheme() == "about" {
        return matches!(url.path(), "blank" | "srcdoc");
    }
    if !plain_https(url) {
        return false;
    }
    let path = url.path();
    match url.host_str() {
        Some(PLAY_HOST) => {
            let page = path
                .strip_prefix("/play/yt/")
                .map(video_id)
                .or_else(|| path.strip_prefix("/play/twitch/").map(twitch_login));
            page == Some(true)
        }
        Some("www.youtube-nocookie.com") => path.starts_with("/embed/"),
        Some("player.twitch.tv") => true,
        _ => false,
    }
}

/// Opens `url` in the system browser if it is a link the feed may open
/// (`model::link`, within `external::check_allowed`).
pub fn open_in_browser<R: Runtime>(app: &AppHandle<R>, url: &str) -> bool {
    use tauri_plugin_opener::OpenerExt;
    if !super::model::link(url) {
        return false;
    }
    match crate::external::check_allowed(url) {
        Ok(url) => app.opener().open_url(url.as_str(), None::<&str>).is_ok(),
        Err(_) => false,
    }
}

/// Shows `url` in the player window: the one window is reused, made on
/// first use. Always queued on the main thread, never inline (CLAUDE.md,
/// "Windows").
pub fn show_player_window<R: Runtime>(app: &AppHandle<R>, url: Url, title: String) {
    if !navigation_allowed(&url) || url.host_str() != Some(PLAY_HOST) {
        return;
    }
    let handle = app.clone();
    crate::lifecycle::queue_on_main_thread(app, move || {
        if let Some(window) = handle.get_webview_window(WINDOW) {
            let _ = window.navigate(url);
            let _ = window.set_title(&title);
            let _ = window.unminimize();
            let _ = window.show();
            let _ = window.set_focus();
            return;
        }
        let opener = handle.clone();
        let built = WebviewWindowBuilder::new(&handle, WINDOW, WebviewUrl::External(url))
            .title(&title)
            .inner_size(960.0, 600.0)
            .min_inner_size(480.0, 300.0)
            // Nothing of the window's pages stays on the computer.
            .incognito(true)
            .disable_drag_drop_handler()
            .on_navigation(navigation_allowed)
            // "Open in browser" and the players' own links: the browser, when
            // the launcher may open the address; never a new window here.
            .on_new_window(move |url, _| {
                open_in_browser(&opener, url.as_str());
                tauri::webview::NewWindowResponse::Deny
            })
            .build();
        match built {
            Ok(window) => {
                let _ = window.set_focus();
            }
            Err(_) => {
                if let Some(state) = handle.try_state::<crate::state::AppState>() {
                    state.console().warn("Feed: the player window did not open");
                }
            }
        }
    });
}
