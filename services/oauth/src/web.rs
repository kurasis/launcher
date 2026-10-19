//! The pages a person uses: `/authorize` (sign-in and registration for the
//! launcher), password reset and email confirmation.
//!
//! A sign-in starts at `/authorize` with the launcher's PKCE challenge. The
//! page carries a random request ID and a CSRF token that must also come back
//! in the `__Host-flow` cookie (SameSite=Strict). A finished sign-in goes to
//! `/app/done?code=…&state=…`, which the launcher's window catches; the code
//! is good once, for a minute, and only with the matching PKCE verifier.

use std::net::IpAddr;
use std::sync::Arc;
use std::time::Duration;

use axum::Form;
use axum::extract::{Query, State};
use axum::http::{HeaderMap, HeaderValue, StatusCode, header};
use axum::response::{IntoResponse, Redirect, Response};
use serde::Deserialize;

use crate::app::{App, CLIENT_ID, CODE_SECONDS, REQUEST_SECONDS, client_country, client_ip, now};
use crate::crypto::{digest, same, token};
use crate::mail::{Letter, queue};
use crate::pages::{self, Field, Form as Page, Lang, texts};
use crate::rules::{self, Problem};
use crate::store::{NewAccount, Request, Taken};

/// HTTPS (every real server): the cookie is `__Host-flow`, `Secure`. Over
/// plain HTTP, only a local test server, browsers drop such a cookie, so it
/// is `flow` there.
static SECURE: std::sync::atomic::AtomicBool = std::sync::atomic::AtomicBool::new(true);

pub fn set_secure(secure: bool) {
    SECURE.store(secure, std::sync::atomic::Ordering::Relaxed);
}

fn cookie_name() -> &'static str {
    if SECURE.load(std::sync::atomic::Ordering::Relaxed) {
        "__Host-flow"
    } else {
        "flow"
    }
}
/// Failed sign-ins from one address in 15 minutes, or for one account, before
/// the captcha is asked.
const IP_FAILURES_FOR_CAPTCHA: u32 = 3;
const ACCOUNT_FAILURES_FOR_CAPTCHA: i64 = 5;
const FAILURE_WINDOW: Duration = Duration::from_secs(15 * 60);

fn cookie(headers: &HeaderMap) -> Option<String> {
    headers
        .get_all(header::COOKIE)
        .iter()
        .filter_map(|v| v.to_str().ok())
        .flat_map(|v| v.split(';'))
        .find_map(|pair| {
            let (name, value) = pair.trim().split_once('=')?;
            (name == cookie_name()).then(|| value.to_owned())
        })
}

fn with_cookie(mut response: Response, csrf: &str) -> Response {
    let secure = if SECURE.load(std::sync::atomic::Ordering::Relaxed) {
        " Secure;"
    } else {
        ""
    };
    if let Ok(value) = HeaderValue::from_str(&format!(
        "{}={csrf}; Path=/;{secure} HttpOnly; SameSite=Strict; Max-Age={REQUEST_SECONDS}",
        cookie_name()
    )) {
        response.headers_mut().append(header::SET_COOKIE, value);
    }
    response
}

/// The CSRF token of the form matches the cookie, and both the one expected.
fn csrf_ok(headers: &HeaderMap, form: &str, expected: &str) -> bool {
    !expected.is_empty()
        && cookie(headers).is_some_and(|c| same(&c, expected))
        && same(form, expected)
}

fn status(status: StatusCode, response: Response) -> Response {
    let mut response = response;
    *response.status_mut() = status;
    response
}

fn expired(lang: Lang) -> Response {
    let t = texts(lang);
    status(
        StatusCode::BAD_REQUEST,
        pages::message(lang, t.title, t.expired),
    )
}

fn failed(lang: Lang) -> Response {
    let t = texts(lang);
    status(
        StatusCode::INTERNAL_SERVER_ERROR,
        pages::message(lang, t.title, t.failed),
    )
}

fn too_many(lang: Lang) -> Response {
    let t = texts(lang);
    status(
        StatusCode::TOO_MANY_REQUESTS,
        pages::message(lang, t.title, t.too_many),
    )
}

#[derive(Deserialize, Default)]
#[serde(default)]
pub struct AuthorizeQuery {
    client_id: String,
    redirect_uri: String,
    response_type: String,
    code_challenge: String,
    code_challenge_method: String,
    state: String,
    device: String,
    lang: String,
    screen: String,
}

/// A PKCE challenge: 43 characters of base64url (S256).
fn challenge_ok(challenge: &str) -> bool {
    challenge.len() == 43
        && challenge
            .bytes()
            .all(|b| b.is_ascii_alphanumeric() || b == b'-' || b == b'_')
}

fn state_ok(state: &str) -> bool {
    !state.is_empty()
        && state.len() <= 128
        && state
            .bytes()
            .all(|b| b.is_ascii_alphanumeric() || b"-._~".contains(&b))
}

pub async fn authorize(
    State(app): State<Arc<App>>,
    headers: HeaderMap,
    Query(q): Query<AuthorizeQuery>,
) -> Response {
    let lang = Lang::parse(&q.lang);
    // Anything else and nothing is redirected anywhere: a wrong address could
    // send the code to someone else.
    if q.client_id != CLIENT_ID
        || q.redirect_uri != app.config.redirect_uri()
        || q.response_type != "code"
        || q.code_challenge_method != "S256"
        || !challenge_ok(&q.code_challenge)
        || !state_ok(&q.state)
    {
        return expired(lang);
    }
    if !app.limiter.allow(client_ip(&headers), "authorize", 120) {
        return too_many(lang);
    }
    let rid = token();
    let request = Request {
        csrf: token(),
        state: q.state,
        challenge: q.code_challenge,
        device: rules::device(&q.device),
        lang: lang.tag().to_owned(),
    };
    if let Err(e) = app
        .store
        .put_request(&digest(&rid), &request, now() + REQUEST_SECONDS)
    {
        eprintln!("storing a sign-in request failed: {e}");
        return failed(lang);
    }
    let csrf = request.csrf.clone();
    let page = if q.screen == "register" {
        register_page(&app, &rid, &request, &RegisterForm::default(), &[])
    } else {
        login_page(&app, &rid, &request, "", &[], false)
    };
    with_cookie(page, &csrf)
}

fn login_page(
    app: &App,
    rid: &str,
    request: &Request,
    login: &str,
    errors: &[&str],
    captcha: bool,
) -> Response {
    let lang = Lang::parse(&request.lang);
    let t = texts(lang);
    let register = format!("/register?rid={rid}");
    let reset = format!("/reset?lang={}", lang.tag());
    pages::form(&Page {
        lang,
        heading: t.sign_in,
        action: "/login",
        hidden: &[("rid", rid), ("csrf", &request.csrf)],
        errors,
        fields: &[
            Field {
                name: "login",
                label: t.login,
                kind: "text",
                value: login,
                hint: None,
                autocomplete: "username",
                max: rules::EMAIL_MAX,
            },
            Field {
                name: "password",
                label: t.password,
                kind: "password",
                value: "",
                hint: None,
                autocomplete: "current-password",
                max: rules::PASSWORD_MAX * 4,
            },
        ],
        submit: t.sign_in,
        turnstile: captcha.then_some((app.config.turnstile_site_key.as_str(), "login")),
        links: &[
            (t.no_account, t.register, &register),
            ("", t.forgot, &reset),
        ],
    })
}

#[derive(Deserialize, Default)]
#[serde(default)]
pub struct RegisterForm {
    rid: String,
    csrf: String,
    nick: String,
    email: String,
    password: String,
    password2: String,
    #[serde(rename = "cf-turnstile-response")]
    turnstile: String,
}

fn register_page(
    app: &App,
    rid: &str,
    request: &Request,
    form: &RegisterForm,
    errors: &[&str],
) -> Response {
    let lang = Lang::parse(&request.lang);
    let t = texts(lang);
    let login = format!("/login?rid={rid}");
    pages::form(&Page {
        lang,
        heading: t.register,
        action: "/register",
        hidden: &[("rid", rid), ("csrf", &request.csrf)],
        errors,
        fields: &[
            Field {
                name: "nick",
                label: t.nick,
                kind: "text",
                value: &form.nick,
                hint: Some(t.nick_hint),
                autocomplete: "username",
                max: rules::NICK_MAX,
            },
            Field {
                name: "email",
                label: t.email,
                kind: "email",
                value: &form.email,
                hint: None,
                autocomplete: "email",
                max: rules::EMAIL_MAX,
            },
            Field {
                name: "password",
                label: t.password,
                kind: "password",
                value: "",
                hint: Some(t.password_hint),
                autocomplete: "new-password",
                max: rules::PASSWORD_MAX * 4,
            },
            Field {
                name: "password2",
                label: t.password_repeat,
                kind: "password",
                value: "",
                hint: None,
                autocomplete: "new-password",
                max: rules::PASSWORD_MAX * 4,
            },
        ],
        submit: t.register,
        turnstile: Some((app.config.turnstile_site_key.as_str(), "register")),
        links: &[(t.have_account, t.sign_in, &login)],
    })
}

#[derive(Deserialize, Default)]
#[serde(default)]
pub struct RidQuery {
    rid: String,
}

/// The sign-in in progress behind a request ID, if it is still open.
fn open_request(app: &App, rid: &str) -> Result<Option<Request>, ()> {
    if rid.len() != 43 {
        return Ok(None);
    }
    app.store.request(&digest(rid), now()).map_err(|e| {
        eprintln!("reading a sign-in request failed: {e}");
    })
}

/// Back and forth between the two screens of one sign-in.
pub async fn login_screen(State(app): State<Arc<App>>, Query(q): Query<RidQuery>) -> Response {
    match open_request(&app, &q.rid) {
        Ok(Some(request)) => {
            let csrf = request.csrf.clone();
            with_cookie(login_page(&app, &q.rid, &request, "", &[], false), &csrf)
        }
        Ok(None) => expired(Lang::En),
        Err(()) => failed(Lang::En),
    }
}

pub async fn register_screen(State(app): State<Arc<App>>, Query(q): Query<RidQuery>) -> Response {
    match open_request(&app, &q.rid) {
        Ok(Some(request)) => {
            let csrf = request.csrf.clone();
            with_cookie(
                register_page(&app, &q.rid, &request, &RegisterForm::default(), &[]),
                &csrf,
            )
        }
        Ok(None) => expired(Lang::En),
        Err(()) => failed(Lang::En),
    }
}

/// Ends the sign-in with a code and sends the window to the launcher's address.
fn finish(app: &App, rid: &str, request: &Request, account: &str) -> Response {
    let lang = Lang::parse(&request.lang);
    let code = token();
    match app.store.finish_request(
        &digest(rid),
        &digest(&code),
        account,
        request,
        now() + CODE_SECONDS,
    ) {
        Ok(true) => {
            let target = format!(
                "{}?code={code}&state={}",
                app.config.redirect_uri(),
                request.state
            );
            Redirect::to(&target).into_response()
        }
        Ok(false) => expired(lang),
        Err(e) => {
            eprintln!("finishing a sign-in failed: {e}");
            failed(lang)
        }
    }
}

#[derive(Deserialize, Default)]
#[serde(default)]
pub struct LoginForm {
    rid: String,
    csrf: String,
    login: String,
    password: String,
    #[serde(rename = "cf-turnstile-response")]
    turnstile: String,
}

pub async fn login(
    State(app): State<Arc<App>>,
    headers: HeaderMap,
    Form(form): Form<LoginForm>,
) -> Response {
    let request = match open_request(&app, &form.rid) {
        Ok(Some(request)) => request,
        Ok(None) => return expired(Lang::En),
        Err(()) => return failed(Lang::En),
    };
    let lang = Lang::parse(&request.lang);
    let t = texts(lang);
    if !csrf_ok(&headers, &form.csrf, &request.csrf) {
        return expired(lang);
    }
    let ip = client_ip(&headers);
    if !app.limiter.allow(ip, "login", 30) {
        return too_many(lang);
    }
    // Many sign-ins from everywhere at once: everyone gets the captcha.
    let crowded = app.limiter.busy("login", 5_000);
    let login = form.login.trim();
    let shown = if login.len() <= rules::EMAIL_MAX {
        login
    } else {
        ""
    };
    let account = if login.is_empty() || login.len() > rules::EMAIL_MAX {
        None
    } else {
        match app.store.find_login(login) {
            Ok(found) => found,
            Err(e) => {
                eprintln!("finding an account failed: {e}");
                return failed(lang);
            }
        }
    };
    let account_failures = account
        .as_ref()
        .and_then(|a| app.store.failures(&a.id, now()).ok())
        .unwrap_or(0);
    // Without captcha keys a captcha cannot be passed: asking for one would
    // lock the account; the rate limits still hold.
    let captcha = app.captcha.configured()
        && (crowded
            || app.limiter.count(ip, "login-failed", FAILURE_WINDOW) >= IP_FAILURES_FOR_CAPTCHA
            || account_failures >= ACCOUNT_FAILURES_FOR_CAPTCHA);
    let page = |errors: &[&str], captcha: bool| {
        with_cookie(
            login_page(&app, &form.rid, &request, shown, errors, captcha),
            &request.csrf,
        )
    };
    if captcha && !app.captcha.verify(&form.turnstile, ip, "login").await {
        return page(&[t.captcha], true);
    }
    let password_fits = form.password.chars().count() <= rules::PASSWORD_MAX;
    let matches = app
        .check(
            if password_fits {
                form.password.clone()
            } else {
                String::new()
            },
            account.as_ref().map(|a| a.password.clone()),
        )
        .await
        && password_fits;
    let Some(account) = account.filter(|_| matches) else {
        app.limiter.note(ip, "login-failed", FAILURE_WINDOW);
        let now_captcha =
            app.limiter.count(ip, "login-failed", FAILURE_WINDOW) >= IP_FAILURES_FOR_CAPTCHA;
        if let Ok(Some(found)) = app
            .store
            .find_login(login)
            .map(|a| a.filter(|_| !login.is_empty()))
        {
            let _ = app.store.fail(&found.id, now());
        }
        return page(&[t.wrong], now_captcha || captcha);
    };
    // Said only to whoever knows the password.
    if account.disabled {
        return page(&[t.disabled], false);
    }
    let _ = app.store.clear_failures(&account.id);
    finish(&app, &form.rid, &request, &account.id)
}

pub async fn register(
    State(app): State<Arc<App>>,
    headers: HeaderMap,
    Form(form): Form<RegisterForm>,
) -> Response {
    let request = match open_request(&app, &form.rid) {
        Ok(Some(request)) => request,
        Ok(None) => return expired(Lang::En),
        Err(()) => return failed(Lang::En),
    };
    let lang = Lang::parse(&request.lang);
    let t = texts(lang);
    if !csrf_ok(&headers, &form.csrf, &request.csrf) {
        return expired(lang);
    }
    let ip = client_ip(&headers);
    if !app.limiter.allow(ip, "register", 5) {
        return too_many(lang);
    }
    // What was typed comes back into the form, cut to what the fields hold.
    let shown = RegisterForm {
        nick: form.nick.chars().take(rules::NICK_MAX).collect(),
        email: form.email.chars().take(rules::EMAIL_MAX).collect(),
        ..RegisterForm::default()
    };
    let page = |errors: &[&str]| {
        with_cookie(
            register_page(&app, &form.rid, &request, &shown, errors),
            &request.csrf,
        )
    };
    if !app.captcha.verify(&form.turnstile, ip, "register").await {
        return page(&[t.captcha]);
    }
    // Only solved captchas count toward everyone's allowance.
    if app.limiter.busy("register", 500) {
        return too_many(lang);
    }
    let nick = rules::nick(&form.nick);
    let email = rules::email(&form.email);
    let mut problems: Vec<Problem> = Vec::new();
    if let Err(p) = &nick {
        problems.push(*p);
    }
    if let Err(p) = &email {
        problems.push(*p);
    }
    if let Err(p) = rules::password(
        &form.password,
        nick.as_deref().unwrap_or_default(),
        email.as_deref().unwrap_or_default(),
    ) {
        problems.push(p);
    }
    if form.password != form.password2 {
        problems.push(Problem::PasswordsDiffer);
    }
    let (Ok(nick), Ok(email), true) = (nick, email, problems.is_empty()) else {
        let texts: Vec<&str> = problems.iter().map(|p| (t.problem)(*p)).collect();
        return page(&texts);
    };
    let Some(hash) = app.hash(form.password.clone()).await else {
        return failed(lang);
    };
    let created = app.store.create_account(&NewAccount {
        nick: &nick,
        email: &email,
        password_hash: &hash,
        lang: lang.tag(),
        ip: &ip.to_string(),
        country: &client_country(&headers),
        now: now(),
    });
    let account = match created {
        Ok(Ok(account)) => account,
        Ok(Err(Taken::Nick)) => return page(&[(t.problem)(Problem::NickTaken)]),
        Ok(Err(Taken::Email)) => return page(&[(t.problem)(Problem::EmailTaken)]),
        Err(e) => {
            eprintln!("creating an account failed: {e}");
            return failed(lang);
        }
    };
    let _ = app
        .store
        .event(&account.id, "registered", &account.reg_country, now());
    send_verification(
        &app,
        &account.id,
        &account.nick,
        &account.email,
        &account.lang,
    );
    finish(&app, &form.rid, &request, &account.id)
}

/// Queues a confirmation letter for the account's current address.
pub fn send_verification(app: &App, account: &str, nick: &str, email: &str, lang: &str) {
    let link_token = token();
    if app
        .store
        .put_email_token(
            &digest(&link_token),
            account,
            "verify",
            email,
            now() + 86_400,
        )
        .is_ok()
    {
        // The page the link opens speaks the account's language.
        let link = format!(
            "{}/verify?token={link_token}&lang={}",
            app.config.base,
            Lang::parse(lang).tag()
        );
        queue(app, email, lang, &Letter::Verify { nick, link: &link });
    }
}

pub async fn done(Query(q): Query<RidQuery>) -> Response {
    // The launcher's window never shows this; a browser that lands here does.
    let _ = q;
    let t = texts(Lang::En);
    pages::message(Lang::En, t.title, t.done)
}

#[derive(Deserialize, Default)]
#[serde(default)]
pub struct TokenQuery {
    token: String,
    lang: String,
}

pub async fn verify(
    State(app): State<Arc<App>>,
    headers: HeaderMap,
    Query(q): Query<TokenQuery>,
) -> Response {
    let lang = Lang::parse(&q.lang);
    let t = texts(lang);
    if !app.limiter.allow(client_ip(&headers), "links", 30) {
        return too_many(lang);
    }
    let hash = digest(&q.token);
    let found = if q.token.len() == 43 {
        app.store.email_token(&hash, "verify", now()).ok().flatten()
    } else {
        None
    };
    match found {
        Some((account, email))
            if app.store.use_email_token(&hash).unwrap_or(false)
                && app.store.verify_email(&account, &email).unwrap_or(false) =>
        {
            let _ = app.store.event(
                &account,
                "email-confirmed",
                &client_country(&headers),
                now(),
            );
            pages::message(lang, t.title, t.verified)
        }
        _ => status(
            StatusCode::BAD_REQUEST,
            pages::message(lang, t.title, t.link_bad),
        ),
    }
}

#[derive(Deserialize, Default)]
#[serde(default)]
pub struct LangQuery {
    lang: String,
}

fn reset_page(app: &App, lang: Lang, csrf: &str, email: &str, errors: &[&str]) -> Response {
    let t = texts(lang);
    with_cookie(
        pages::form(&Page {
            lang,
            heading: t.reset_title,
            action: "/reset",
            hidden: &[("csrf", csrf), ("lang", lang.tag())],
            errors,
            fields: &[Field {
                name: "email",
                label: t.email,
                kind: "email",
                value: email,
                hint: Some(t.reset_text),
                autocomplete: "email",
                max: rules::EMAIL_MAX,
            }],
            submit: t.reset_send,
            turnstile: Some((app.config.turnstile_site_key.as_str(), "reset")),
            links: &[],
        }),
        csrf,
    )
}

pub async fn reset_form(State(app): State<Arc<App>>, Query(q): Query<LangQuery>) -> Response {
    reset_page(&app, Lang::parse(&q.lang), &token(), "", &[])
}

#[derive(Deserialize, Default)]
#[serde(default)]
pub struct ResetForm {
    csrf: String,
    lang: String,
    email: String,
    #[serde(rename = "cf-turnstile-response")]
    turnstile: String,
}

pub async fn reset(
    State(app): State<Arc<App>>,
    headers: HeaderMap,
    Form(form): Form<ResetForm>,
) -> Response {
    let lang = Lang::parse(&form.lang);
    let t = texts(lang);
    let Some(csrf) = cookie(&headers).filter(|c| c.len() == 43 && same(c, &form.csrf)) else {
        return expired(lang);
    };
    let ip = client_ip(&headers);
    if !app.limiter.allow(ip, "reset", 5) {
        return too_many(lang);
    }
    let shown: String = form.email.chars().take(rules::EMAIL_MAX).collect();
    if !app.captcha.verify(&form.turnstile, ip, "reset").await {
        return reset_page(&app, lang, &csrf, &shown, &[t.captcha]);
    }
    if app.limiter.busy("reset", 1_000) {
        return too_many(lang);
    }
    let Ok(email) = rules::email(&form.email) else {
        return reset_page(&app, lang, &csrf, &shown, &[(t.problem)(Problem::Email)]);
    };
    // The same answer whether or not the address has an account.
    if let Ok(Some(account)) = app.store.find_login(&email) {
        let recent = app
            .store
            .count_events(&account.id, "reset-asked", now() - 3600)
            .unwrap_or(i64::MAX);
        if !account.disabled && recent < 3 {
            let link_token = token();
            if app
                .store
                .put_email_token(
                    &digest(&link_token),
                    &account.id,
                    "reset",
                    &account.email,
                    now() + 30 * 60,
                )
                .is_ok()
            {
                let link = format!(
                    "{}/reset/confirm?token={link_token}&lang={}",
                    app.config.base,
                    lang.tag()
                );
                queue(
                    &app,
                    &account.email,
                    &account.lang,
                    &Letter::Reset {
                        nick: &account.nick,
                        link: &link,
                    },
                );
                let _ =
                    app.store
                        .event(&account.id, "reset-asked", &client_country(&headers), now());
            }
        }
    }
    pages::message(lang, t.reset_title, t.reset_sent)
}

fn new_password_page(lang: Lang, csrf: &str, link_token: &str, errors: &[&str]) -> Response {
    let t = texts(lang);
    with_cookie(
        pages::form(&Page {
            lang,
            heading: t.reset_title,
            action: "/reset/confirm",
            hidden: &[("csrf", csrf), ("token", link_token), ("lang", lang.tag())],
            errors,
            fields: &[
                Field {
                    name: "password",
                    label: t.reset_new,
                    kind: "password",
                    value: "",
                    hint: Some(t.password_hint),
                    autocomplete: "new-password",
                    max: rules::PASSWORD_MAX * 4,
                },
                Field {
                    name: "password2",
                    label: t.password_repeat,
                    kind: "password",
                    value: "",
                    hint: None,
                    autocomplete: "new-password",
                    max: rules::PASSWORD_MAX * 4,
                },
            ],
            submit: t.reset_save,
            turnstile: None,
            links: &[],
        }),
        csrf,
    )
}

pub async fn reset_confirm_form(
    State(app): State<Arc<App>>,
    headers: HeaderMap,
    Query(q): Query<TokenQuery>,
) -> Response {
    let lang = Lang::parse(&q.lang);
    let t = texts(lang);
    if !app.limiter.allow(client_ip(&headers), "links", 30) {
        return too_many(lang);
    }
    let valid = q.token.len() == 43
        && matches!(
            app.store.email_token(&digest(&q.token), "reset", now()),
            Ok(Some(_))
        );
    if !valid {
        return status(
            StatusCode::BAD_REQUEST,
            pages::message(lang, t.reset_title, t.link_bad),
        );
    }
    new_password_page(lang, &token(), &q.token, &[])
}

#[derive(Deserialize, Default)]
#[serde(default)]
pub struct NewPasswordForm {
    csrf: String,
    token: String,
    lang: String,
    password: String,
    password2: String,
}

pub async fn reset_confirm(
    State(app): State<Arc<App>>,
    headers: HeaderMap,
    Form(form): Form<NewPasswordForm>,
) -> Response {
    let lang = Lang::parse(&form.lang);
    let t = texts(lang);
    let Some(csrf) = cookie(&headers).filter(|c| c.len() == 43 && same(c, &form.csrf)) else {
        return expired(lang);
    };
    let ip: IpAddr = client_ip(&headers);
    if !app.limiter.allow(ip, "links", 30) {
        return too_many(lang);
    }
    let hash = digest(&form.token);
    let found = if form.token.len() == 43 {
        app.store.email_token(&hash, "reset", now()).ok().flatten()
    } else {
        None
    };
    let Some((account_id, sent_to)) = found else {
        return status(
            StatusCode::BAD_REQUEST,
            pages::message(lang, t.reset_title, t.link_bad),
        );
    };
    let Ok(Some(account)) = app.store.account(&account_id) else {
        return failed(lang);
    };
    // A link sent to an address the account no longer has does nothing.
    if rules::email_key(&sent_to) != rules::email_key(&account.email) {
        return status(
            StatusCode::BAD_REQUEST,
            pages::message(lang, t.reset_title, t.link_bad),
        );
    }
    let mut problems = Vec::new();
    if let Err(p) = rules::password(&form.password, &account.nick, &account.email) {
        problems.push((t.problem)(p));
    }
    if form.password != form.password2 {
        problems.push((t.problem)(Problem::PasswordsDiffer));
    }
    if !problems.is_empty() {
        return new_password_page(lang, &csrf, &form.token, &problems);
    }
    let Some(new_hash) = app.hash(form.password.clone()).await else {
        return failed(lang);
    };
    if !app.store.use_email_token(&hash).unwrap_or(false) {
        return status(
            StatusCode::BAD_REQUEST,
            pages::message(lang, t.reset_title, t.link_bad),
        );
    }
    if app.store.set_password(&account.id, &new_hash).is_err() {
        return failed(lang);
    }
    let _ = app.store.revoke_all(&account.id, None);
    let _ = app.store.clear_failures(&account.id);
    let _ = app.store.event(
        &account.id,
        "password-reset",
        &client_country(&headers),
        now(),
    );
    queue(
        &app,
        &account.email,
        &account.lang,
        &Letter::PasswordChanged {
            nick: &account.nick,
        },
    );
    pages::message(lang, t.reset_title, t.reset_done)
}

pub async fn style() -> Response {
    (
        [
            (header::CONTENT_TYPE, "text/css; charset=utf-8"),
            (header::CACHE_CONTROL, "public, max-age=3600"),
            (header::X_CONTENT_TYPE_OPTIONS, "nosniff"),
        ],
        pages::STYLE,
    )
        .into_response()
}

// Deleting the account (D-064 stage 5). The password is typed here, on our
// page, never in the launcher; a second, one-time step confirms.

fn delete_page(app: &App, lang: Lang, csrf: &str, login: &str, errors: &[&str]) -> Response {
    let t = texts(lang);
    // A rare step that proves a password: always behind the captcha when it
    // can be solved.
    let turnstile = app
        .captcha
        .configured()
        .then_some((app.config.turnstile_site_key.as_str(), "delete"));
    with_cookie(
        pages::form(&Page {
            lang,
            heading: t.delete_title,
            action: "/delete",
            hidden: &[("csrf", csrf), ("lang", lang.tag())],
            errors,
            fields: &[
                Field {
                    name: "login",
                    label: t.login,
                    kind: "text",
                    value: login,
                    hint: Some(t.delete_text),
                    autocomplete: "username",
                    max: rules::EMAIL_MAX,
                },
                Field {
                    name: "password",
                    label: t.password,
                    kind: "password",
                    value: "",
                    hint: None,
                    autocomplete: "current-password",
                    max: rules::PASSWORD_MAX,
                },
            ],
            submit: t.delete_next,
            turnstile,
            links: &[],
        }),
        csrf,
    )
}

pub async fn delete_form(State(app): State<Arc<App>>, Query(q): Query<LangQuery>) -> Response {
    delete_page(&app, Lang::parse(&q.lang), &token(), "", &[])
}

#[derive(Deserialize, Default)]
#[serde(default)]
pub struct DeleteForm {
    csrf: String,
    lang: String,
    login: String,
    password: String,
    #[serde(rename = "cf-turnstile-response")]
    turnstile: String,
}

/// Checks the password like a sign-in, then asks once more.
pub async fn delete(
    State(app): State<Arc<App>>,
    headers: HeaderMap,
    Form(form): Form<DeleteForm>,
) -> Response {
    let lang = Lang::parse(&form.lang);
    let t = texts(lang);
    let Some(csrf) = cookie(&headers).filter(|c| c.len() == 43 && same(c, &form.csrf)) else {
        return expired(lang);
    };
    let ip = client_ip(&headers);
    if !app.limiter.allow(ip, "delete", 10) {
        return too_many(lang);
    }
    let login = form.login.trim();
    let shown = if login.len() <= rules::EMAIL_MAX {
        login
    } else {
        ""
    };
    if app.captcha.configured() && !app.captcha.verify(&form.turnstile, ip, "delete").await {
        return delete_page(&app, lang, &csrf, shown, &[t.captcha]);
    }
    let account = if login.is_empty() || login.len() > rules::EMAIL_MAX {
        None
    } else {
        app.store.find_login(login).ok().flatten()
    };
    let password_fits = form.password.chars().count() <= rules::PASSWORD_MAX;
    let matches = app
        .check(
            if password_fits {
                form.password.clone()
            } else {
                String::new()
            },
            account.as_ref().map(|a| a.password.clone()),
        )
        .await
        && password_fits;
    let Some(account) = account.filter(|_| matches) else {
        app.limiter.note(ip, "login-failed", FAILURE_WINDOW);
        if let Ok(Some(found)) = app.store.find_login(login) {
            let _ = app.store.fail(&found.id, now());
        }
        return delete_page(&app, lang, &csrf, shown, &[t.wrong]);
    };
    let confirm = token();
    if app
        .store
        .put_email_token(
            &digest(&confirm),
            &account.id,
            "delete",
            &account.email,
            now() + 10 * 60,
        )
        .is_err()
    {
        return failed(lang);
    }
    let heading = format!("{} {}", t.delete_title, account.nick);
    with_cookie(
        pages::form(&Page {
            lang,
            heading: &heading,
            action: "/delete/confirm",
            hidden: &[("csrf", &csrf), ("lang", lang.tag()), ("token", &confirm)],
            errors: &[t.delete_confirm],
            fields: &[],
            submit: t.delete_submit,
            turnstile: None,
            links: &[],
        }),
        &csrf,
    )
}

#[derive(Deserialize, Default)]
#[serde(default)]
pub struct DeleteConfirmForm {
    csrf: String,
    lang: String,
    token: String,
}

/// Deletes the account, tells its address, and goes to `/app/deleted`, where
/// the launcher's window learns of it and forgets the account.
pub async fn delete_confirm(
    State(app): State<Arc<App>>,
    headers: HeaderMap,
    Form(form): Form<DeleteConfirmForm>,
) -> Response {
    let lang = Lang::parse(&form.lang);
    let t = texts(lang);
    if cookie(&headers)
        .filter(|c| c.len() == 43 && same(c, &form.csrf))
        .is_none()
    {
        return expired(lang);
    }
    if !app.limiter.allow(client_ip(&headers), "links", 30) {
        return too_many(lang);
    }
    let hash = digest(&form.token);
    let found = if form.token.len() == 43 {
        app.store.email_token(&hash, "delete", now()).ok().flatten()
    } else {
        None
    };
    let bad = || {
        status(
            StatusCode::BAD_REQUEST,
            pages::message(lang, t.delete_title, t.link_bad),
        )
    };
    let Some((account_id, _)) = found else {
        return bad();
    };
    if !app.store.use_email_token(&hash).unwrap_or(false) {
        return bad();
    }
    let Ok(Some(account)) = app.store.account(&account_id) else {
        return failed(lang);
    };
    if !app
        .store
        .delete_account(&account.id, now())
        .unwrap_or(false)
    {
        return failed(lang);
    }
    queue(
        &app,
        &account.email,
        &account.lang,
        &Letter::Deleted {
            nick: &account.nick,
        },
    );
    Redirect::to(&format!("/app/deleted?lang={}", lang.tag())).into_response()
}

pub async fn deleted(Query(q): Query<LangQuery>) -> Response {
    let lang = Lang::parse(&q.lang);
    let t = texts(lang);
    pages::message(lang, t.delete_title, t.deleted)
}
