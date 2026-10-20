//! The feed API (D-067). Everything is public and read-only, with no
//! account; nothing about a player (an address, their channels) is written
//! to the log.

use std::collections::HashMap;
use std::sync::{Arc, Mutex};
use std::time::{Duration, Instant};

use axum::body::Body;
use axum::extract::{DefaultBodyLimit, Path, Query, State};
use axum::http::{HeaderMap, HeaderValue, StatusCode, header};
use axum::response::{IntoResponse, Response};
use axum::routing::{get, post};
use axum::{Json, Router};
use serde::Deserialize;
use serde_json::{Value, json};

use crate::feed::{self, Feed};
use crate::images::{twitch_login, video_id};
use crate::mojang;
use crate::youtube;

/// Channels a player may have of their own.
pub const MAX_OWN: usize = 30;
/// Channel lookups per address a minute (each one asks YouTube).
pub const RESOLVES_PER_MINUTE: u32 = 10;
/// Requests for a player's own videos per address a minute.
pub const OWN_PER_MINUTE: u32 = 30;

const CACHE_FEED: &str = "public, max-age=60";
const CACHE_NOTE: &str = "public, max-age=600";
const CACHE_PICTURE: &str = "public, max-age=31536000, immutable";
const CACHE_PREVIEW: &str = "public, max-age=86400";
const CACHE_STREAM: &str = "public, max-age=300";

/// Actions counted since the start of their window, per address and kind.
type Windows = HashMap<(String, &'static str), (Instant, u32)>;

/// Actions per address in a window.
#[derive(Default)]
pub struct Limiter {
    windows: Mutex<Windows>,
}

impl Limiter {
    pub fn allow(&self, key: &str, bucket: &'static str, limit: u32, window: Duration) -> bool {
        let mut windows = self
            .windows
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner);
        let now = Instant::now();
        if windows.len() > 100_000 {
            windows.retain(|_, (start, _)| now.duration_since(*start) < window);
            if windows.len() > 500_000 {
                windows.clear();
            }
        }
        let entry = windows.entry((key.to_owned(), bucket)).or_insert((now, 0));
        if now.duration_since(entry.0) >= window {
            *entry = (now, 0);
        }
        if entry.1 >= limit {
            return false;
        }
        entry.1 += 1;
        true
    }
}

pub struct App {
    pub feed: Feed,
    pub limiter: Limiter,
}

fn refuse(status: StatusCode, code: &str) -> Response {
    (status, Json(json!({ "error": code }))).into_response()
}

fn cached(mut response: Response, policy: &'static str) -> Response {
    response
        .headers_mut()
        .insert(header::CACHE_CONTROL, HeaderValue::from_static(policy));
    response
}

/// The address nginx saw (an IPv6 /64 counts as one).
fn address(headers: &HeaderMap) -> String {
    let raw = headers
        .get("x-real-ip")
        .and_then(|v| v.to_str().ok())
        .unwrap_or("local");
    match raw.parse::<std::net::IpAddr>() {
        Ok(std::net::IpAddr::V6(v6)) => {
            let s = v6.segments();
            format!("{:x}:{:x}:{:x}:{:x}::/64", s[0], s[1], s[2], s[3])
        }
        Ok(ip) => ip.to_string(),
        Err(_) => "local".to_owned(),
    }
}

#[derive(Deserialize)]
struct LangQuery {
    lang: Option<String>,
}

async fn get_feed(State(app): State<Arc<App>>, Query(q): Query<LangQuery>) -> Response {
    let lang = feed::lang(q.lang.as_deref());
    cached(Json(app.feed.build(lang)).into_response(), CACHE_FEED)
}

async fn get_note(
    State(app): State<Arc<App>>,
    Path(id): Path<String>,
    Query(q): Query<LangQuery>,
) -> Response {
    if !mojang::note_id(&id) {
        return refuse(StatusCode::NOT_FOUND, "not_found");
    }
    match app.feed.note(&id, feed::lang(q.lang.as_deref())) {
        Some(note) => cached(Json(note).into_response(), CACHE_NOTE),
        None => refuse(StatusCode::NOT_FOUND, "not_found"),
    }
}

async fn picture(app: &App, kind: &'static str, name: &str) -> Response {
    let stream_url = match kind {
        "tw" => name
            .strip_suffix(".jpg")
            .filter(|l| twitch_login(l))
            .and_then(|login| app.feed.stream_preview(login)),
        "yt" => {
            // Only previews of videos some channel we fetched has: no one
            // makes us fetch arbitrary pictures.
            if !name
                .strip_suffix(".jpg")
                .is_some_and(|id| video_id(id) && app.feed.video_known(id))
            {
                return refuse(StatusCode::NOT_FOUND, "not_found");
            }
            None
        }
        _ => None,
    };
    let Some(path) = app.feed.images.file(kind, name, stream_url).await else {
        return refuse(StatusCode::NOT_FOUND, "not_found");
    };
    let Ok(bytes) = tokio::fs::read(&path).await else {
        return refuse(StatusCode::NOT_FOUND, "not_found");
    };
    let policy = match kind {
        "img" => CACHE_PICTURE,
        "yt" => CACHE_PREVIEW,
        _ => CACHE_STREAM,
    };
    let mut response = Response::new(Body::from(bytes));
    let headers = response.headers_mut();
    headers.insert(header::CONTENT_TYPE, HeaderValue::from_static("image/jpeg"));
    headers.insert(header::CACHE_CONTROL, HeaderValue::from_static(policy));
    response
}

async fn get_img(State(app): State<Arc<App>>, Path(name): Path<String>) -> Response {
    picture(&app, "img", &name).await
}

async fn get_yt(State(app): State<Arc<App>>, Path(name): Path<String>) -> Response {
    picture(&app, "yt", &name).await
}

async fn get_tw(State(app): State<Arc<App>>, Path(name): Path<String>) -> Response {
    picture(&app, "tw", &name).await
}

#[derive(Deserialize)]
struct Resolve {
    input: String,
}

async fn resolve(
    State(app): State<Arc<App>>,
    headers: HeaderMap,
    body: Result<Json<Resolve>, axum::extract::rejection::JsonRejection>,
) -> Response {
    let Ok(Json(body)) = body else {
        return refuse(StatusCode::BAD_REQUEST, "bad_request");
    };
    let Some(wanted) = youtube::wanted(&body.input) else {
        return refuse(StatusCode::BAD_REQUEST, "bad_input");
    };
    if !app.limiter.allow(
        &address(&headers),
        "resolve",
        RESOLVES_PER_MINUTE,
        Duration::from_secs(60),
    ) {
        return refuse(StatusCode::TOO_MANY_REQUESTS, "too_many");
    }
    match youtube::resolve(&app.feed.http, wanted).await {
        Ok((id, channel)) => {
            let title = channel.title.clone();
            app.feed.keep_own(&id, channel);
            cached(
                Json(json!({ "channelId": id, "title": title })).into_response(),
                "no-store",
            )
        }
        Err(_) => refuse(StatusCode::NOT_FOUND, "not_found"),
    }
}

#[derive(Deserialize)]
struct VideosQuery {
    channels: Option<String>,
}

async fn own_videos(
    State(app): State<Arc<App>>,
    headers: HeaderMap,
    Query(q): Query<VideosQuery>,
) -> Response {
    let raw = q.channels.unwrap_or_default();
    let mut ids: Vec<String> = Vec::new();
    for id in raw.split(',').filter(|s| !s.is_empty()) {
        if !youtube::channel_id(id) || ids.len() >= MAX_OWN {
            return refuse(StatusCode::BAD_REQUEST, "bad_channels");
        }
        if !ids.iter().any(|i| i == id) {
            ids.push(id.to_owned());
        }
    }
    if !app.limiter.allow(
        &address(&headers),
        "own",
        OWN_PER_MINUTE,
        Duration::from_secs(60),
    ) {
        return refuse(StatusCode::TOO_MANY_REQUESTS, "too_many");
    }
    let videos = app.feed.own_videos(&ids).await;
    cached(
        Json(json!({ "videos": videos })).into_response(),
        "public, max-age=300",
    )
}

/// The player window's page: the video or stream in the provider's embedded
/// player, and a link to open it in the browser. A spinner over the frame
/// shows that the player is loading; the page's one script, allowed by its
/// hash alone, takes the spinner away when the frame has loaded.
const PLAY_SCRIPT: &str = "document.querySelector('iframe').addEventListener('load',function(){document.body.className='ready'})";
/// `PLAY_SCRIPT`'s SHA-256 in base64, for the page's CSP.
const PLAY_SCRIPT_HASH: &str = "rPflnP8K0ltC1LurZvLdOaueT4w+lPFlCGYuRJcIkLs=";

fn play_page(lang: &str, title: &str, frame: &str, open: &str) -> Response {
    let (open_text, dir) = match lang {
        "ru" => ("Открыть в браузере", "ltr"),
        "ar" => ("فتح في المتصفح", "rtl"),
        _ => ("Open in browser", "ltr"),
    };
    let html = format!(
        "<!doctype html><html lang=\"{lang}\" dir=\"{dir}\"><head><meta charset=\"utf-8\">\
<meta name=\"viewport\" content=\"width=device-width, initial-scale=1\">\
<meta name=\"referrer\" content=\"strict-origin-when-cross-origin\"><title>{title}</title>\
<style>html,body{{margin:0;height:100%;background:#000;color:#ddd;font:14px system-ui,sans-serif}}\
body{{display:flex;flex-direction:column}}main{{flex:1;position:relative}}\
iframe{{position:absolute;inset:0;width:100%;height:100%;border:0}}\
.wait{{position:absolute;z-index:1;pointer-events:none;transition:opacity .3s,visibility .3s;top:50%;left:50%;width:44px;height:44px;margin:-22px 0 0 -22px;box-sizing:border-box;\
border:3px solid #2a2a2a;border-top-color:#9cf;border-radius:50%;animation:spin .9s linear infinite}}\
.ready .wait{{opacity:0;visibility:hidden}}@keyframes spin{{to{{transform:rotate(1turn)}}}}@media (prefers-reduced-motion:reduce){{.wait{{animation-duration:3s}}}}\
a{{color:#9cf;padding:6px 12px;text-align:end;text-decoration:none}}a:hover{{text-decoration:underline}}</style></head>\
<body><main><iframe src=\"{frame}\" title=\"{title}\" allow=\"autoplay; encrypted-media; picture-in-picture; fullscreen\" \
allowfullscreen referrerpolicy=\"strict-origin-when-cross-origin\"></iframe>\
<div class=\"wait\" aria-hidden=\"true\"></div></main><script>{PLAY_SCRIPT}</script>\
<a href=\"{open}\" target=\"_blank\" rel=\"noopener noreferrer\">{open_text}</a></body></html>"
    );
    let mut response = Response::new(Body::from(html));
    let headers = response.headers_mut();
    headers.insert(
        header::CONTENT_TYPE,
        HeaderValue::from_static("text/html; charset=utf-8"),
    );
    headers.insert(
        header::CONTENT_SECURITY_POLICY,
        HeaderValue::from_str(&format!(
            "default-src 'none'; frame-src https://www.youtube-nocookie.com https://player.twitch.tv; \
             script-src 'sha256-{PLAY_SCRIPT_HASH}'; style-src 'unsafe-inline'; base-uri 'none'; \
             form-action 'none'; frame-ancestors 'none'"
        ))
        .unwrap_or_else(|_| HeaderValue::from_static("default-src 'none'")),
    );
    // YouTube's embedded player refuses to play without the page's origin.
    headers.insert(
        header::REFERRER_POLICY,
        HeaderValue::from_static("strict-origin-when-cross-origin"),
    );
    headers.insert(
        header::CACHE_CONTROL,
        HeaderValue::from_static("public, max-age=86400"),
    );
    response
}

async fn play_youtube(Path(id): Path<String>, Query(q): Query<LangQuery>) -> Response {
    if !video_id(&id) {
        return refuse(StatusCode::NOT_FOUND, "not_found");
    }
    play_page(
        feed::lang(q.lang.as_deref()),
        "YouTube",
        &format!("https://www.youtube-nocookie.com/embed/{id}?autoplay=1&amp;rel=0"),
        &format!("https://www.youtube.com/watch?v={id}"),
    )
}

async fn play_twitch(Path(login): Path<String>, Query(q): Query<LangQuery>) -> Response {
    if !twitch_login(&login) {
        return refuse(StatusCode::NOT_FOUND, "not_found");
    }
    play_page(
        feed::lang(q.lang.as_deref()),
        "Twitch",
        &format!(
            "https://player.twitch.tv/?channel={login}&amp;parent=feed.lapi.sh&amp;autoplay=true"
        ),
        &format!("https://www.twitch.tv/{login}"),
    )
}

async fn health(State(app): State<Arc<App>>) -> Json<Value> {
    Json(json!({
        "service": "lapi-feed",
        "ok": true,
        "version": env!("CARGO_PKG_VERSION"),
        "counts": app.feed.counts(),
    }))
}

async fn not_found() -> Response {
    refuse(StatusCode::NOT_FOUND, "not_found")
}

pub fn router(app: Arc<App>) -> Router {
    Router::new()
        .route("/v1/health", get(health))
        .route("/v1/feed", get(get_feed))
        .route("/v1/notes/{id}", get(get_note))
        .route("/v1/img/{name}", get(get_img))
        .route("/v1/yt/{name}", get(get_yt))
        .route("/v1/tw/{name}", get(get_tw))
        .route("/v1/channels/resolve", post(resolve))
        .route("/v1/videos", get(own_videos))
        .route("/play/yt/{id}", get(play_youtube))
        .route("/play/twitch/{login}", get(play_twitch))
        .fallback(not_found)
        .layer(DefaultBodyLimit::max(4 * 1024))
        .with_state(app)
}
