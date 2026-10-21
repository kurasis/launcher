//! The system notification that a favourite Twitch channel went live
//! (D-067): only when the player turned it on in the feed's settings, and
//! only while the launcher is not in front (in front, the feed shows the
//! stream itself). Each stream is told once, by its channel and start.

use std::collections::HashSet;
use std::sync::Arc;
use std::time::Duration;

use tauri::{AppHandle, Manager, Runtime};

use super::FeedService;
use super::model::Stream;
use crate::state::AppState;

/// How often the favourite channels are looked at.
pub const EVERY: Duration = Duration::from_secs(180);

/// At most this many streams are told at one look; the rest only noted.
pub const MAX_TOLD: usize = 3;

/// The streams seen so far are forgotten past this many.
const MAX_SEEN: usize = 500;

/// The streams seen live, by channel and start.
#[derive(Debug, Default)]
pub struct Seen {
    streams: HashSet<(String, String)>,
    primed: bool,
}

impl Seen {
    /// The streams that went live since the last look. The first look only
    /// learns what is live already, so starting the launcher (or turning the
    /// notifications on) tells nothing.
    pub fn newly_live<'a>(&mut self, live: &'a [Stream]) -> Vec<&'a Stream> {
        if self.streams.len() > MAX_SEEN {
            self.streams.clear();
            self.primed = false;
        }
        let primed = self.primed;
        self.primed = true;
        live.iter()
            .filter(|stream| {
                self.streams
                    .insert((stream.login.clone(), stream.started.clone()))
                    && primed
            })
            .collect()
    }

    /// Starts again, as if nothing had been seen.
    pub fn forget(&mut self) {
        *self = Self::default();
    }
}

/// "is live on Twitch" in the launcher's language (the notification is made
/// here, without the interface).
pub fn live_words(language: &str) -> &'static str {
    match language.get(..2).unwrap_or_default() {
        "ru" => "в эфире на Twitch",
        "ar" => "يبث الآن على Twitch",
        _ => "is live on Twitch",
    }
}

/// Looks at the favourite channels every [`EVERY`] for as long as the
/// launcher runs.
pub async fn watch<R: Runtime>(app: AppHandle<R>, feed: Arc<FeedService>) {
    let mut seen = Seen::default();
    loop {
        tokio::time::sleep(EVERY).await;
        look(&app, &feed, &mut seen).await;
    }
}

async fn look<R: Runtime>(app: &AppHandle<R>, feed: &FeedService, seen: &mut Seen) {
    let state = app.state::<AppState>();
    let settings = state.settings();
    if !settings.feed.enabled || !settings.feed.live_notifications {
        seen.forget();
        return;
    }
    let Some(root) = state.data_root() else {
        return;
    };
    let Ok(live) = feed.favorite_streams(&root).await else {
        return;
    };
    let new = seen.newly_live(&live);
    if new.is_empty() || in_front(app) {
        return;
    }
    use tauri_plugin_notification::NotificationExt as _;
    let words = live_words(settings.language.as_str());
    for stream in new.into_iter().take(MAX_TOLD) {
        let name = crate::friends::notification_text(&stream.name, 40);
        let _ = app
            .notification()
            .builder()
            .title(format!("{name} {words}"))
            .body(crate::friends::notification_text(&stream.title, 160))
            .show();
    }
}

fn in_front<R: Runtime>(app: &AppHandle<R>) -> bool {
    app.get_webview_window(crate::lifecycle::MAIN_WINDOW)
        .is_some_and(|w| w.is_focused().unwrap_or(false) && !w.is_minimized().unwrap_or(true))
}

#[cfg(test)]
mod tests {
    use super::*;

    fn stream(login: &str, started: &str) -> Stream {
        Stream {
            login: login.into(),
            name: login.into(),
            title: "Hardcore day 1".into(),
            viewers: 10,
            started: started.into(),
            image: None,
        }
    }

    fn logins(found: Vec<&Stream>) -> Vec<&str> {
        found.into_iter().map(|s| s.login.as_str()).collect()
    }

    #[test]
    fn a_stream_is_told_once_and_not_at_the_first_look() {
        let mut seen = Seen::default();
        let first = [stream("alpha", "2026-10-21T10:00:00Z")];
        assert!(seen.newly_live(&first).is_empty());
        let second = [
            stream("alpha", "2026-10-21T10:00:00Z"),
            stream("beta", "2026-10-21T10:05:00Z"),
        ];
        assert_eq!(logins(seen.newly_live(&second)), ["beta"]);
        assert!(seen.newly_live(&second).is_empty());
        // Gone a moment and back with the same start: not told again.
        assert!(seen.newly_live(&[]).is_empty());
        assert!(seen.newly_live(&second).is_empty());
    }

    #[test]
    fn a_new_start_of_the_same_channel_is_told() {
        let mut seen = Seen::default();
        assert!(seen.newly_live(&[]).is_empty());
        let morning = [stream("alpha", "2026-10-21T08:00:00Z")];
        let evening = [stream("alpha", "2026-10-21T19:00:00Z")];
        assert_eq!(logins(seen.newly_live(&morning)), ["alpha"]);
        assert_eq!(logins(seen.newly_live(&evening)), ["alpha"]);
    }

    #[test]
    fn forgetting_starts_again_quietly() {
        let mut seen = Seen::default();
        assert!(seen.newly_live(&[]).is_empty());
        seen.forget();
        let live = [stream("alpha", "2026-10-21T10:00:00Z")];
        assert!(seen.newly_live(&live).is_empty());
    }

    #[test]
    fn what_is_seen_stays_bounded() {
        let mut seen = Seen::default();
        let many: Vec<Stream> = (0..=MAX_SEEN)
            .map(|n| stream(&format!("user{n}"), "2026-10-21T10:00:00Z"))
            .collect();
        seen.newly_live(&many);
        assert!(seen.newly_live(&many[..1]).is_empty());
        assert!(seen.streams.len() <= MAX_SEEN);
    }

    #[test]
    fn the_words_follow_the_language() {
        assert_eq!(live_words("ru-RU"), "в эфире на Twitch");
        assert_eq!(live_words("ar"), "يبث الآن على Twitch");
        assert_eq!(live_words("pt-BR"), "is live on Twitch");
        assert_eq!(live_words(""), "is live on Twitch");
    }
}
