//! The internal API for friends.lapi.sh (D-064): who has a nick, and the
//! nicks of account IDs. It listens on its own local address
//! (`LAPI_OAUTH_INTERNAL_ADDR`, default `127.0.0.1:8793`), which nginx does not
//! serve, so nothing outside the server reaches it. It tells only what anyone
//! can see in the launcher: a nick and the account it names.

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

pub fn router(app: Arc<App>) -> Router {
    Router::new()
        .route("/internal/v1/find", get(find))
        .route("/internal/v1/nicks", post(nicks))
        .layer(axum::extract::DefaultBodyLimit::max(64 * 1024))
        .with_state(app)
}
