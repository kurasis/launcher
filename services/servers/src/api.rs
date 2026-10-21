//! The API at servers.lapi.sh (D-069): read only. It has no route that takes
//! a body or changes anything; ads are written by the owner's commands on the
//! server alone. Nothing about a player is written to the log.

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
use crate::icons::{Icons, sha_name};
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
    let live = match app.store.live(place, now) {
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
    let Some(sha) = file.strip_suffix(".png").filter(|s| sha_name(s)) else {
        return refuse(StatusCode::NOT_FOUND, "not_found");
    };
    match app.icons.read(sha) {
        Some(bytes) => {
            let mut response = Response::new(Body::from(bytes));
            response
                .headers_mut()
                .insert(header::CONTENT_TYPE, HeaderValue::from_static("image/png"));
            cached(response, CACHE_ICON)
        }
        None => refuse(StatusCode::NOT_FOUND, "not_found"),
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
        .fallback(not_found)
        // No route reads a body.
        .layer(DefaultBodyLimit::max(0))
        .with_state(app)
}
