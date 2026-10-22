//! The API at servers.lapi.sh (D-069): read only. It has no route that takes
//! a body or changes anything; ads are written by the owner's commands on the
//! server alone. Nothing about a player is written to the log.
//!
//! `/v1/online/{id}` is for the statistics service on this server (D-071):
//! nginx does not pass it on, so it cannot be asked from outside.

use std::sync::Arc;

use axum::body::Body;
use axum::extract::{DefaultBodyLimit, Path, Query, State};
use axum::http::{HeaderValue, StatusCode, header};
use axum::response::{IntoResponse, Response};
use axum::routing::get;
use axum::{Json, Router};
use serde::Deserialize;
use serde_json::json;

use crate::ads;
use crate::icons::{Icons, Kind, sha_name};
use crate::store::Store;
use crate::text;

const CACHE_ADS: &str = "public, max-age=30";
const CACHE_ICON: &str = "public, max-age=31536000, immutable";

pub struct App {
    pub store: Store,
    pub icons: Icons,
    /// Unix seconds now; tests set their own.
    pub clock: fn() -> i64,
}

pub fn now() -> i64 {
    chrono::Utc::now().timestamp()
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

#[derive(Deserialize)]
struct AdsQuery {
    place: Option<String>,
    lang: Option<String>,
}

async fn get_ads(State(app): State<Arc<App>>, Query(q): Query<AdsQuery>) -> Response {
    let Ok(place) = text::place(q.place.as_deref().unwrap_or("feed")) else {
        return refuse(StatusCode::NOT_FOUND, "unknown_place");
    };
    let lang = ads::lang(q.lang.as_deref());
    let now = (app.clock)();
    let live = match app.store.live(place, lang, now) {
        Ok(live) => live,
        Err(e) => {
            eprintln!("ads: {e}");
            return refuse(StatusCode::SERVICE_UNAVAILABLE, "unavailable");
        }
    };
    let mut list = Vec::with_capacity(live.len());
    for ad in &live {
        match ads::public(&app.store, ad, lang) {
            Ok(value) => list.push(value),
            Err(e) => eprintln!("ad {}: {e}", ad.id),
        }
    }
    cached(
        Json(json!({ "now": now, "ads": list })).into_response(),
        CACHE_ADS,
    )
}

async fn get_icon(State(app): State<Arc<App>>, Path(file): Path<String>) -> Response {
    // Icons are PNGs, banners JPEGs.
    let found = [Kind::Png, Kind::Jpeg].into_iter().find_map(|kind| {
        file.strip_suffix(kind.ext())
            .and_then(|s| s.strip_suffix('.'))
            .filter(|s| sha_name(s))
            .map(|sha| (sha, kind))
    });
    let Some((sha, kind)) = found else {
        return refuse(StatusCode::NOT_FOUND, "not_found");
    };
    match app.icons.read(sha, kind) {
        Some(bytes) => {
            let mut response = Response::new(Body::from(bytes));
            response
                .headers_mut()
                .insert(header::CONTENT_TYPE, HeaderValue::from_static(kind.mime()));
            cached(response, CACHE_ICON)
        }
        None => refuse(StatusCode::NOT_FOUND, "not_found"),
    }
}

/// Hours of history the statistics service gets of an ad.
const ONLINE_DAYS: i64 = 120;

/// An ad's players online by hour, with when it starts and ends.
async fn get_online(State(app): State<Arc<App>>, Path(id): Path<String>) -> Response {
    let Some(id) = (!id.is_empty() && id.len() <= 12 && id.bytes().all(|b| b.is_ascii_digit()))
        .then(|| id.parse::<i64>().ok())
        .flatten()
    else {
        return refuse(StatusCode::NOT_FOUND, "not_found");
    };
    let now = (app.clock)();
    let found = app.store.ad(id).and_then(|ad| {
        ad.map(|ad| {
            app.store
                .online(id, now - ONLINE_DAYS * 86_400)
                .map(|hours| (ad, hours))
        })
        .transpose()
    });
    match found {
        Ok(Some((ad, hours))) => Json(json!({
            "ad": id.to_string(),
            "starts": ad.starts,
            "ends": ad.ends,
            "hours": hours,
        }))
        .into_response(),
        Ok(None) => refuse(StatusCode::NOT_FOUND, "not_found"),
        Err(e) => {
            eprintln!("online: {e}");
            refuse(StatusCode::SERVICE_UNAVAILABLE, "unavailable")
        }
    }
}

async fn health() -> Json<serde_json::Value> {
    Json(json!({
        "service": "lapi-servers",
        "ok": true,
        "version": env!("CARGO_PKG_VERSION"),
    }))
}

async fn not_found() -> Response {
    refuse(StatusCode::NOT_FOUND, "not_found")
}

pub fn router(app: Arc<App>) -> Router {
    Router::new()
        .route("/v1/health", get(health))
        .route("/v1/ads", get(get_ads))
        .route("/v1/icons/{file}", get(get_icon))
        .route("/v1/online/{id}", get(get_online))
        .fallback(not_found)
        // No route reads a body.
        .layer(DefaultBodyLimit::max(0))
        .with_state(app)
}
