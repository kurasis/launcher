//! The JSON API for the launcher: tokens (`/v1/token`, `/v1/revoke`) and the
//! account itself (`/v1/me…`, with an access token).

use std::sync::Arc;

use axum::extract::{DefaultBodyLimit, Request, State};
use axum::http::{HeaderMap, StatusCode, header};
use axum::middleware::{self, Next};
use axum::response::{IntoResponse, Response};
use axum::routing::{get, post};
use axum::{Form, Json, Router};
use serde::Deserialize;
use serde_json::json;

use crate::app::{ACCESS_SECONDS, App, CLIENT_ID, SESSION_SECONDS, client_country, client_ip, now};
use crate::crypto::{Claims, digest, pkce_matches, token, verify};
use crate::mail::{self, Letter, queue};
use crate::rules::{self, Problem};
use crate::store::{Account, Refresh, Taken};
use crate::web;

/// An OAuth error (RFC 6749): `invalid_grant`, `invalid_request`…
fn oauth_error(status: StatusCode, error: &str) -> Response {
    (status, Json(json!({ "error": error }))).into_response()
}

/// An API error with a code the launcher translates.
fn refuse(status: StatusCode, code: &str) -> Response {
    (status, Json(json!({ "error": code }))).into_response()
}

fn problem_code(problem: Problem) -> &'static str {
    match problem {
        Problem::NickLength => "nick_length",
        Problem::NickCharacters => "nick_characters",
        Problem::NickReserved => "nick_reserved",
        Problem::NickTaken => "nick_taken",
        Problem::Email => "email",
        Problem::EmailTaken => "email_taken",
        Problem::PasswordShort => "password_short",
        Problem::PasswordLong => "password_long",
        Problem::PasswordCommon => "password_common",
        Problem::PasswordLikeNick => "password_like_nick",
        Problem::PasswordsDiffer => "passwords_differ",
    }
}

fn account_json(account: &Account) -> serde_json::Value {
    json!({
        "id": account.id,
        "nick": account.nick,
        "email": account.email,
        "emailVerified": account.email_verified,
        "created": account.created,
    })
}

/// A fresh access token for a session.
fn access_token(app: &App, account: &Account, session: &str) -> String {
    let now = now();
    app.keys.sign(&Claims {
        iss: app.config.base.clone(),
        sub: account.id.clone(),
        aud: vec!["account".into(), "friends".into(), "mods".into()],
        iat: now,
        exp: now + ACCESS_SECONDS,
        sid: session.to_owned(),
        nick: account.nick.clone(),
        ev: account.email_verified,
    })
}

fn tokens(app: &App, account: &Account, session: &str, refresh: &str) -> Response {
    let mut response = Json(json!({
        "access_token": access_token(app, account, session),
        "token_type": "Bearer",
        "expires_in": ACCESS_SECONDS,
        "refresh_token": refresh,
        "account": { "id": account.id, "nick": account.nick, "emailVerified": account.email_verified },
    }))
    .into_response();
    response
        .headers_mut()
        .insert(header::CACHE_CONTROL, "no-store".parse().unwrap());
    response
}

#[derive(Deserialize, Default)]
#[serde(default)]
pub struct TokenForm {
    grant_type: String,
    code: String,
    code_verifier: String,
    client_id: String,
    redirect_uri: String,
    refresh_token: String,
}

async fn token_endpoint(
    State(app): State<Arc<App>>,
    headers: HeaderMap,
    Form(form): Form<TokenForm>,
) -> Response {
    if !app.limiter.allow(client_ip(&headers), "token", 240) {
        return oauth_error(StatusCode::TOO_MANY_REQUESTS, "slow_down");
    }
    if form.client_id != CLIENT_ID {
        return oauth_error(StatusCode::UNAUTHORIZED, "invalid_client");
    }
    let country = client_country(&headers);
    let now = now();
    match form.grant_type.as_str() {
        "authorization_code" => {
            if form.redirect_uri != app.config.redirect_uri() || form.code.len() != 43 {
                return oauth_error(StatusCode::BAD_REQUEST, "invalid_grant");
            }
            let Ok(Some((account_id, challenge, device))) =
                app.store.take_code(&digest(&form.code), now)
            else {
                return oauth_error(StatusCode::BAD_REQUEST, "invalid_grant");
            };
            if !pkce_matches(&form.code_verifier, &challenge) {
                return oauth_error(StatusCode::BAD_REQUEST, "invalid_grant");
            }
            let Ok(Some(account)) = app.store.account(&account_id) else {
                return oauth_error(StatusCode::BAD_REQUEST, "invalid_grant");
            };
            if account.disabled {
                return oauth_error(StatusCode::BAD_REQUEST, "invalid_grant");
            }
            let refresh = token();
            let Ok(session) = app.store.create_session(
                &account.id,
                &device,
                &country,
                &digest(&refresh),
                now,
                now + SESSION_SECONDS,
            ) else {
                return oauth_error(StatusCode::INTERNAL_SERVER_ERROR, "server_error");
            };
            let _ = app.store.event(&account.id, "signed-in", &country, now);
            tokens(&app, &account, &session, &refresh)
        }
        "refresh_token" => {
            if form.refresh_token.len() != 43 {
                return oauth_error(StatusCode::BAD_REQUEST, "invalid_grant");
            }
            let next = token();
            match app.store.rotate(
                &digest(&form.refresh_token),
                &digest(&next),
                now,
                now + SESSION_SECONDS,
            ) {
                Ok(Refresh::Fresh { session, account }) => match app.store.account(&account) {
                    Ok(Some(account)) => tokens(&app, &account, &session, &next),
                    _ => oauth_error(StatusCode::BAD_REQUEST, "invalid_grant"),
                },
                Ok(Refresh::Reused { account }) => {
                    let _ = app.store.event(&account, "refresh-reused", &country, now);
                    oauth_error(StatusCode::BAD_REQUEST, "invalid_grant")
                }
                Ok(Refresh::Invalid) => oauth_error(StatusCode::BAD_REQUEST, "invalid_grant"),
                Err(e) => {
                    eprintln!("refreshing failed: {e}");
                    oauth_error(StatusCode::INTERNAL_SERVER_ERROR, "server_error")
                }
            }
        }
        _ => oauth_error(StatusCode::BAD_REQUEST, "unsupported_grant_type"),
    }
}

#[derive(Deserialize, Default)]
#[serde(default)]
pub struct RevokeForm {
    token: String,
}

/// Signs this session out (RFC 7009: the answer is the same either way).
async fn revoke(
    State(app): State<Arc<App>>,
    headers: HeaderMap,
    Form(form): Form<RevokeForm>,
) -> Response {
    if !app.limiter.allow(client_ip(&headers), "token", 240) {
        return oauth_error(StatusCode::TOO_MANY_REQUESTS, "slow_down");
    }
    if form.token.len() == 43
        && let Ok(Some(account)) = app.store.revoke_by_refresh(&digest(&form.token))
    {
        let _ = app
            .store
            .event(&account, "signed-out", &client_country(&headers), now());
    }
    StatusCode::OK.into_response()
}

/// The account and session of a request's access token, if both are live.
struct Me {
    account: Account,
    session: String,
}

fn me(app: &App, headers: &HeaderMap) -> Result<Me, Box<Response>> {
    let unauthorized = || Box::new(refuse(StatusCode::UNAUTHORIZED, "unauthorized"));
    let bearer = headers
        .get(header::AUTHORIZATION)
        .and_then(|v| v.to_str().ok())
        .and_then(|v| v.strip_prefix("Bearer "))
        .ok_or_else(unauthorized)?;
    let claims = verify(
        &app.keys.public(),
        bearer.trim(),
        &app.config.base,
        "account",
        now(),
    )
    .ok_or_else(unauthorized)?;
    if !app
        .store
        .session_alive(&claims.sid, &claims.sub, now())
        .unwrap_or(false)
    {
        return Err(unauthorized());
    }
    let account = app
        .store
        .account(&claims.sub)
        .ok()
        .flatten()
        .ok_or_else(unauthorized)?;
    Ok(Me {
        account,
        session: claims.sid,
    })
}

async fn get_me(State(app): State<Arc<App>>, headers: HeaderMap) -> Response {
    match me(&app, &headers) {
        Ok(me) => Json(account_json(&me.account)).into_response(),
        Err(response) => *response,
    }
}

#[derive(Deserialize)]
struct NickBody {
    nick: String,
}

async fn set_nick(
    State(app): State<Arc<App>>,
    headers: HeaderMap,
    Json(body): Json<NickBody>,
) -> Response {
    let me = match me(&app, &headers) {
        Ok(me) => me,
        Err(response) => return *response,
    };
    let nick = match rules::nick(&body.nick) {
        Ok(nick) => nick,
        Err(p) => return refuse(StatusCode::UNPROCESSABLE_ENTITY, problem_code(p)),
    };
    // Changes are not limited in time, only against a script renaming in a loop.
    if app
        .store
        .count_events(&me.account.id, "nick-changed", now() - 86_400)
        .unwrap_or(i64::MAX)
        >= 30
    {
        return refuse(StatusCode::TOO_MANY_REQUESTS, "too_many");
    }
    match app.store.set_nick(&me.account.id, &nick) {
        Ok(Ok(())) => {
            let _ = app.store.event(
                &me.account.id,
                "nick-changed",
                &client_country(&headers),
                now(),
            );
            let account = Account { nick, ..me.account };
            Json(json!({
                "account": account_json(&account),
                "access_token": access_token(&app, &account, &me.session),
                "expires_in": ACCESS_SECONDS,
            }))
            .into_response()
        }
        Ok(Err(_)) => refuse(StatusCode::CONFLICT, problem_code(Problem::NickTaken)),
        Err(e) => {
            eprintln!("changing a nick failed: {e}");
            refuse(StatusCode::INTERNAL_SERVER_ERROR, "server_error")
        }
    }
}

#[derive(Deserialize)]
struct PasswordBody {
    current: String,
    new: String,
}

async fn set_password(
    State(app): State<Arc<App>>,
    headers: HeaderMap,
    Json(body): Json<PasswordBody>,
) -> Response {
    let me = match me(&app, &headers) {
        Ok(me) => me,
        Err(response) => return *response,
    };
    if !app.limiter.allow(client_ip(&headers), "password", 10) {
        return refuse(StatusCode::TOO_MANY_REQUESTS, "too_many");
    }
    if body.current.chars().count() > rules::PASSWORD_MAX
        || !app
            .check(body.current.clone(), Some(me.account.password.clone()))
            .await
    {
        return refuse(StatusCode::FORBIDDEN, "wrong_password");
    }
    if let Err(p) = rules::password(&body.new, &me.account.nick, &me.account.email) {
        return refuse(StatusCode::UNPROCESSABLE_ENTITY, problem_code(p));
    }
    let Some(hash) = app.hash(body.new.clone()).await else {
        return refuse(StatusCode::INTERNAL_SERVER_ERROR, "server_error");
    };
    if app.store.set_password(&me.account.id, &hash).is_err() {
        return refuse(StatusCode::INTERNAL_SERVER_ERROR, "server_error");
    }
    let _ = app.store.revoke_all(&me.account.id, Some(&me.session));
    let _ = app.store.event(
        &me.account.id,
        "password-changed",
        &client_country(&headers),
        now(),
    );
    queue(
        &app,
        &me.account.email,
        &me.account.lang,
        &Letter::PasswordChanged {
            nick: &me.account.nick,
        },
    );
    StatusCode::NO_CONTENT.into_response()
}

#[derive(Deserialize)]
struct EmailBody {
    email: String,
    password: String,
}

/// A new address: needs the password, and is confirmed by a letter to it.
async fn set_email(
    State(app): State<Arc<App>>,
    headers: HeaderMap,
    Json(body): Json<EmailBody>,
) -> Response {
    let me = match me(&app, &headers) {
        Ok(me) => me,
        Err(response) => return *response,
    };
    if !app.limiter.allow(client_ip(&headers), "password", 10) {
        return refuse(StatusCode::TOO_MANY_REQUESTS, "too_many");
    }
    let email = match rules::email(&body.email) {
        Ok(email) => email,
        Err(p) => return refuse(StatusCode::UNPROCESSABLE_ENTITY, problem_code(p)),
    };
    if body.password.chars().count() > rules::PASSWORD_MAX
        || !app
            .check(body.password.clone(), Some(me.account.password.clone()))
            .await
    {
        return refuse(StatusCode::FORBIDDEN, "wrong_password");
    }
    match app.store.set_email(&me.account.id, &email) {
        Ok(Ok(())) => {
            // The old address learns of the change, so a stolen password
            // cannot take the account's recovery quietly.
            if rules::email_key(&me.account.email) != rules::email_key(&email) {
                mail::queue(
                    &app,
                    &me.account.email,
                    &me.account.lang,
                    &Letter::EmailChanged {
                        nick: &me.account.nick,
                        new: &mail::masked(&email),
                    },
                );
            }
            let _ = app.store.event(
                &me.account.id,
                "email-changed",
                &client_country(&headers),
                now(),
            );
            web::send_verification(
                &app,
                &me.account.id,
                &me.account.nick,
                &email,
                &me.account.lang,
            );
            StatusCode::NO_CONTENT.into_response()
        }
        Ok(Err(Taken::Email)) => refuse(StatusCode::CONFLICT, problem_code(Problem::EmailTaken)),
        Ok(Err(Taken::Nick)) | Err(_) => refuse(StatusCode::INTERNAL_SERVER_ERROR, "server_error"),
    }
}

/// Everything kept about the account, for the account itself (its own
/// session asks; the launcher saves it as a file).
async fn export(State(app): State<Arc<App>>, headers: HeaderMap) -> Response {
    let me = match me(&app, &headers) {
        Ok(me) => me,
        Err(response) => return *response,
    };
    if !app.limiter.allow(client_ip(&headers), "export", 10) {
        return refuse(StatusCode::TOO_MANY_REQUESTS, "too_many");
    }
    match app.store.export(&me.account.id, now()) {
        Ok(data) => Json(data).into_response(),
        Err(_) => refuse(StatusCode::INTERNAL_SERVER_ERROR, "server_error"),
    }
}

/// Sends the confirmation letter again, at most three times an hour.
async fn resend_verification(State(app): State<Arc<App>>, headers: HeaderMap) -> Response {
    let me = match me(&app, &headers) {
        Ok(me) => me,
        Err(response) => return *response,
    };
    if me.account.email_verified {
        return StatusCode::NO_CONTENT.into_response();
    }
    if app
        .store
        .count_events(&me.account.id, "verification-sent", now() - 3600)
        .unwrap_or(i64::MAX)
        >= 3
    {
        return refuse(StatusCode::TOO_MANY_REQUESTS, "too_many");
    }
    let _ = app.store.event(
        &me.account.id,
        "verification-sent",
        &client_country(&headers),
        now(),
    );
    web::send_verification(
        &app,
        &me.account.id,
        &me.account.nick,
        &me.account.email,
        &me.account.lang,
    );
    StatusCode::NO_CONTENT.into_response()
}

async fn sessions(State(app): State<Arc<App>>, headers: HeaderMap) -> Response {
    let me = match me(&app, &headers) {
        Ok(me) => me,
        Err(response) => return *response,
    };
    match app.store.sessions(&me.account.id, now()) {
        Ok(list) => Json(json!({
            "sessions": list.iter().map(|s| json!({
                "id": s.id,
                "device": s.device,
                "country": s.country,
                "created": s.created,
                "lastUsed": s.last_used,
                "current": s.id == me.session,
            })).collect::<Vec<_>>()
        }))
        .into_response(),
        Err(_) => refuse(StatusCode::INTERNAL_SERVER_ERROR, "server_error"),
    }
}

#[derive(Deserialize)]
struct SessionBody {
    id: String,
}

async fn end_session(
    State(app): State<Arc<App>>,
    headers: HeaderMap,
    Json(body): Json<SessionBody>,
) -> Response {
    let me = match me(&app, &headers) {
        Ok(me) => me,
        Err(response) => return *response,
    };
    match app.store.revoke_session(&me.account.id, &body.id) {
        Ok(true) => StatusCode::NO_CONTENT.into_response(),
        Ok(false) => refuse(StatusCode::NOT_FOUND, "not_found"),
        Err(_) => refuse(StatusCode::INTERNAL_SERVER_ERROR, "server_error"),
    }
}

async fn end_other_sessions(State(app): State<Arc<App>>, headers: HeaderMap) -> Response {
    let me = match me(&app, &headers) {
        Ok(me) => me,
        Err(response) => return *response,
    };
    match app.store.revoke_all(&me.account.id, Some(&me.session)) {
        Ok(_) => {
            let _ = app.store.event(
                &me.account.id,
                "signed-out-others",
                &client_country(&headers),
                now(),
            );
            StatusCode::NO_CONTENT.into_response()
        }
        Err(_) => refuse(StatusCode::INTERNAL_SERVER_ERROR, "server_error"),
    }
}

async fn jwks(State(app): State<Arc<App>>) -> Response {
    (
        [(header::CACHE_CONTROL, "public, max-age=300")],
        Json(app.keys.jwks()),
    )
        .into_response()
}

async fn health() -> Json<serde_json::Value> {
    Json(json!({ "ok": true, "service": "lapi-oauth", "version": env!("CARGO_PKG_VERSION") }))
}

/// The account API's overall request limit per address, before any body is
/// read.
async fn admit(State(app): State<Arc<App>>, request: Request, next: Next) -> Response {
    if request.uri().path().starts_with("/v1/me")
        && !app.limiter.allow(client_ip(request.headers()), "me", 600)
    {
        return refuse(StatusCode::TOO_MANY_REQUESTS, "too_many");
    }
    next.run(request).await
}

/// Every route: the pages and the API.
pub fn router(app: Arc<App>) -> Router {
    Router::new()
        .route("/v1/health", get(health))
        .route("/.well-known/jwks.json", get(jwks))
        .route("/v1/token", post(token_endpoint))
        .route("/v1/revoke", post(revoke))
        .route("/v1/me", get(get_me))
        .route("/v1/me/nick", post(set_nick))
        .route("/v1/me/password", post(set_password))
        .route("/v1/me/email", post(set_email))
        .route("/v1/me/verify-email", post(resend_verification))
        .route("/v1/me/sessions", get(sessions))
        .route("/v1/me/export", get(export))
        .route("/v1/me/sessions/end", post(end_session))
        .route("/v1/me/sessions/end-others", post(end_other_sessions))
        .route("/authorize", get(web::authorize))
        .route("/login", get(web::login_screen).post(web::login))
        .route("/register", get(web::register_screen).post(web::register))
        .route("/app/done", get(web::done))
        .route("/delete", get(web::delete_form).post(web::delete))
        .route("/delete/confirm", post(web::delete_confirm))
        .route("/app/deleted", get(web::deleted))
        .route("/verify", get(web::verify))
        .route("/reset", get(web::reset_form).post(web::reset))
        .route(
            "/reset/confirm",
            get(web::reset_confirm_form).post(web::reset_confirm),
        )
        .route("/static/style.css", get(web::style))
        // Forms and JSON bodies are small; nothing bigger is read.
        .layer(DefaultBodyLimit::max(16 * 1024))
        .layer(middleware::from_fn_with_state(Arc::clone(&app), admit))
        .with_state(app)
}
