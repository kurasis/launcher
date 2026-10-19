//! The internal API for friends.lapi.sh (D-064) and skins.lapi.sh (D-065):
//! who has a nick, the nicks of account IDs, which accounts were deleted,
//! whether letters go out, and which sessions ended (so a sign-out or a
//! switched-off account reaches them within seconds, not when the token runs
//! out). It listens on its own local address (`LAPI_OAUTH_INTERNAL_ADDR`,
//! default `127.0.0.1:8793`), which nginx does not serve, so nothing outside
//! the server reaches it. It tells only what anyone can see in the launcher: a
//! nick and the account it names.

use std::collections::BTreeMap;
use std::sync::Arc;

use axum::extract::{Query, State};
use axum::http::StatusCode;
use axum::response::{IntoResponse, Response};
use axum::routing::{get, post};
use axum::{Json, Router};
use serde::Deserialize;
use serde_json::json;

use crate::app::App;
use crate::rules;

/// IDs per question.
const MAX_IDS: usize = 500;

#[derive(Deserialize, Default)]
#[serde(default)]
struct FindQuery {
    nick: String,
}

/// The account with this nick (exactly, without regard to case).
async fn find(State(app): State<Arc<App>>, Query(q): Query<FindQuery>) -> Response {
    let Ok(nick) = rules::nick(&q.nick) else {
        return StatusCode::NOT_FOUND.into_response();
    };
    match app.store.find_login(&nick) {
        Ok(Some(account)) if !account.disabled => {
            Json(json!({ "id": account.id, "nick": account.nick })).into_response()
        }
        Ok(_) => StatusCode::NOT_FOUND.into_response(),
        Err(_) => StatusCode::INTERNAL_SERVER_ERROR.into_response(),
    }
}

#[derive(Deserialize)]
struct NicksBody {
    ids: Vec<String>,
}

/// The current nick of each account that exists and is not switched off.
async fn nicks(State(app): State<Arc<App>>, Json(body): Json<NicksBody>) -> Response {
    if body.ids.len() > MAX_IDS {
        return StatusCode::PAYLOAD_TOO_LARGE.into_response();
    }
    let mut found = BTreeMap::new();
    for id in body
        .ids
        .iter()
        .filter(|id| uuid::Uuid::parse_str(id).is_ok())
    {
        match app.store.account(id) {
            Ok(Some(account)) if !account.disabled => {
                found.insert(account.id, account.nick);
            }
            Ok(_) => {}
            Err(_) => return StatusCode::INTERNAL_SERVER_ERROR.into_response(),
        }
    }
    Json(json!({ "nicks": found })).into_response()
}

#[derive(Deserialize)]
struct SessionsBody {
    sessions: Vec<String>,
}

/// Of the sessions asked about, those that no longer work.
async fn ended(State(app): State<Arc<App>>, Json(body): Json<SessionsBody>) -> Response {
    if body.sessions.len() > MAX_IDS {
        return StatusCode::PAYLOAD_TOO_LARGE.into_response();
    }
    match app.store.ended_sessions(&body.sessions, crate::app::now()) {
        Ok(ended) => Json(json!({ "ended": ended })).into_response(),
        Err(_) => StatusCode::INTERNAL_SERVER_ERROR.into_response(),
    }
}

#[derive(Deserialize, Default)]
#[serde(default)]
struct SinceQuery {
    since: i64,
}

/// Accounts deleted after `since`, for friends.lapi.sh to clean out.
async fn deleted(State(app): State<Arc<App>>, Query(q): Query<SinceQuery>) -> Response {
    match app.store.deleted_since(q.since) {
        Ok(list) => {
            let deleted: Vec<_> = list
                .into_iter()
                .map(|(id, at)| json!({ "id": id, "at": at }))
                .collect();
            Json(json!({ "deleted": deleted })).into_response()
        }
        Err(_) => StatusCode::INTERNAL_SERVER_ERROR.into_response(),
    }
}

/// Whether letters go out (an SMTP server is set): friends asks for a
/// confirmed address only then.
async fn mail(State(app): State<Arc<App>>) -> Response {
    Json(json!({ "sending": app.mailing })).into_response()
}

pub fn router(app: Arc<App>) -> Router {
    Router::new()
        .route("/internal/v1/find", get(find))
        .route("/internal/v1/nicks", post(nicks))
        .route("/internal/v1/ended", post(ended))
        .route("/internal/v1/deleted", get(deleted))
        .route("/internal/v1/mail", get(mail))
        .layer(axum::extract::DefaultBodyLimit::max(64 * 1024))
        .with_state(app)
}
