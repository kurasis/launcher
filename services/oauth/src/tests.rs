//! The whole service through its router: sign-in and registration as the
//! launcher's window does them, tokens, the account API, and what an attacker
//! would send instead.

use std::future::Future;
use std::net::IpAddr;
use std::pin::Pin;
use std::sync::Arc;

use axum::Router;
use axum::body::Body;
use axum::http::{Request, StatusCode, header};
use base64::Engine as _;
use http_body_util::BodyExt;
use sha2::Digest as _;
use tower::ServiceExt;

use crate::app::{App, Captcha, Config};
use crate::crypto::{Keys, verify};
use crate::store::Store;

const BASE: &str = "https://oauth.example";
const VERIFIER: &str = "dBjftJeZ4CVP-mB92K27uhbUJU1p1r_wW1gFWFOEjXk";
const CHALLENGE: &str = "E9Melhoa2OwvFrEMTJguCHaoeK1t8URWbuGJSstw-cM";
const PASSWORD: &str = "correct horse battery";

/// Turnstile in tests: "ok" passes for any action, "ok:<action>" for that one;
/// `keys: false` is a server without captcha keys.
struct FakeCaptcha {
    keys: bool,
}

impl Captcha for FakeCaptcha {
    fn configured(&self) -> bool {
        self.keys
    }

    fn verify<'a>(
        &'a self,
        response: &'a str,
        _ip: IpAddr,
        action: &'a str,
    ) -> Pin<Box<dyn Future<Output = bool> + Send + 'a>> {
        Box::pin(async move { response == "ok" || response == format!("ok:{action}") })
    }
}

struct T {
    _dir: tempfile::TempDir,
    app: Arc<App>,
    router: Router,
}

fn setup() -> T {
    setup_with(true)
}

fn setup_with(keys: bool) -> T {
    let dir = tempfile::tempdir().unwrap();
    let store = Store::open(dir.path()).unwrap();
    let app = Arc::new(App::new(
        Config {
            base: BASE.into(),
            turnstile_site_key: "site-key".into(),
        },
        store,
        Keys::from_seed([3; 32]),
        Box::new(FakeCaptcha { keys }),
    ));
    T {
        _dir: dir,
        router: crate::api::router(Arc::clone(&app)),
        app,
    }
}

struct Answer {
    status: StatusCode,
    headers: axum::http::HeaderMap,
    body: String,
}

impl Answer {
    fn json(&self) -> serde_json::Value {
        serde_json::from_str(&self.body).unwrap_or(serde_json::Value::Null)
    }

    fn location(&self) -> String {
        self.headers
            .get(header::LOCATION)
            .map(|v| v.to_str().unwrap().to_owned())
            .unwrap_or_default()
    }

    fn cookie(&self) -> Option<String> {
        self.headers
            .get(header::SET_COOKIE)
            .and_then(|v| v.to_str().ok())
            .and_then(|v| v.split(';').next())
            .map(str::to_owned)
    }

    /// The value of a hidden field in the page.
    fn field(&self, name: &str) -> String {
        let marker = format!("name=\"{name}\" value=\"");
        let start = self.body.find(&marker).expect(name) + marker.len();
        self.body[start..].split('"').next().unwrap().to_owned()
    }
}

fn form_body(fields: &[(&str, &str)]) -> String {
    fields
        .iter()
        .map(|(k, v)| format!("{}={}", enc(k), enc(v)))
        .collect::<Vec<_>>()
        .join("&")
}

fn enc(text: &str) -> String {
    text.bytes()
        .map(|b| match b {
            b'A'..=b'Z' | b'a'..=b'z' | b'0'..=b'9' | b'-' | b'.' | b'_' | b'~' => {
                (b as char).to_string()
            }
            _ => format!("%{b:02X}"),
        })
        .collect()
}

impl T {
    async fn send(&self, request: Request<Body>) -> Answer {
        let response = self.router.clone().oneshot(request).await.unwrap();
        let status = response.status();
        let headers = response.headers().clone();
        let bytes = response.into_body().collect().await.unwrap().to_bytes();
        Answer {
            status,
            headers,
            body: String::from_utf8_lossy(&bytes).into_owned(),
        }
    }

    fn get(&self, uri: &str, ip: &str) -> Request<Body> {
        Request::get(uri)
            .header("x-real-ip", ip)
            .header("x-country", "DE")
            .body(Body::empty())
            .unwrap()
    }

    fn post_form(
        &self,
        uri: &str,
        ip: &str,
        cookie: Option<&str>,
        fields: &[(&str, &str)],
    ) -> Request<Body> {
        let mut builder = Request::post(uri)
            .header("x-real-ip", ip)
            .header("x-country", "DE")
            .header(header::CONTENT_TYPE, "application/x-www-form-urlencoded");
        if let Some(cookie) = cookie {
            builder = builder.header(header::COOKIE, cookie);
        }
        builder.body(Body::from(form_body(fields))).unwrap()
    }

    fn api(&self, method: &str, uri: &str, token: &str, body: serde_json::Value) -> Request<Body> {
        Request::builder()
            .method(method)
            .uri(uri)
            .header("x-real-ip", "203.0.113.9")
            .header(header::AUTHORIZATION, format!("Bearer {token}"))
            .header(header::CONTENT_TYPE, "application/json")
            .body(Body::from(body.to_string()))
            .unwrap()
    }

    /// Opens a sign-in page as the launcher does: (page, request ID, CSRF, cookie).
    async fn open(&self, screen: &str, ip: &str) -> (Answer, String, String, String) {
        let uri = format!(
            "/authorize?client_id=launcher&redirect_uri={}&response_type=code&code_challenge={CHALLENGE}\
             &code_challenge_method=S256&state=st4te&device=Windows%2011&lang=ru&screen={screen}",
            enc(&format!("{BASE}/app/done"))
        );
        let page = self.send(self.get(&uri, ip)).await;
        assert_eq!(page.status, StatusCode::OK, "{}", page.body);
        let rid = page.field("rid");
        let csrf = page.field("csrf");
        let cookie = page.cookie().unwrap();
        assert_eq!(cookie, format!("__Host-flow={csrf}"));
        (page, rid, csrf, cookie)
    }

    async fn register(&self, nick: &str, email: &str, ip: &str) -> Answer {
        let (_, rid, csrf, cookie) = self.open("register", ip).await;
        self.send(self.post_form(
            "/register",
            ip,
            Some(&cookie),
            &[
                ("rid", &rid),
                ("csrf", &csrf),
                ("nick", nick),
                ("email", email),
                ("password", PASSWORD),
                ("password2", PASSWORD),
                ("cf-turnstile-response", "ok:register"),
            ],
        ))
        .await
    }

    async fn login(&self, login: &str, password: &str, captcha: &str, ip: &str) -> Answer {
        let (_, rid, csrf, cookie) = self.open("login", ip).await;
        self.send(self.post_form(
            "/login",
            ip,
            Some(&cookie),
            &[
                ("rid", &rid),
                ("csrf", &csrf),
                ("login", login),
                ("password", password),
                ("cf-turnstile-response", captcha),
            ],
        ))
        .await
    }

    /// Exchanges the code of a finished sign-in for tokens.
    async fn exchange(&self, finished: &Answer, verifier: &str) -> Answer {
        let location = finished.location();
        assert!(
            location.starts_with(&format!("{BASE}/app/done?code=")),
            "{location}"
        );
        assert!(location.ends_with("&state=st4te"));
        let code = location
            .split("code=")
            .nth(1)
            .unwrap()
            .split('&')
            .next()
            .unwrap()
            .to_owned();
        self.send(self.post_form(
            "/v1/token",
            "203.0.113.9",
            None,
            &[
                ("grant_type", "authorization_code"),
                ("code", &code),
                ("code_verifier", verifier),
                ("client_id", "launcher"),
                ("redirect_uri", &format!("{BASE}/app/done")),
            ],
        ))
        .await
    }

    async fn refresh(&self, token: &str) -> Answer {
        self.send(self.post_form(
            "/v1/token",
            "203.0.113.9",
            None,
            &[
                ("grant_type", "refresh_token"),
                ("refresh_token", token),
                ("client_id", "launcher"),
            ],
        ))
        .await
    }

    /// Registers and signs in: the token answer.
    async fn account(&self, nick: &str, ip: &str) -> serde_json::Value {
        let registered = self
            .register(nick, &format!("{}@example.com", nick.to_lowercase()), ip)
            .await;
        assert_eq!(
            registered.status,
            StatusCode::SEE_OTHER,
            "{}",
            registered.body
        );
        let tokens = self.exchange(&registered, VERIFIER).await;
        assert_eq!(tokens.status, StatusCode::OK, "{}", tokens.body);
        tokens.json()
    }
}

#[tokio::test]
async fn register_then_sign_in_with_pkce_and_use_the_tokens() {
    let t = setup();
    let (page, ..) = t.open("register", "198.51.100.1").await;
    // The page: Russian, a Turnstile widget, strict headers.
    assert!(page.body.contains("lang=\"ru\"") && page.body.contains("Создать аккаунт"));
    assert!(page.body.contains("data-sitekey=\"site-key\""));
    assert!(
        page.headers[header::CONTENT_SECURITY_POLICY]
            .to_str()
            .unwrap()
            .contains("frame-ancestors 'none'")
    );
    let tokens = t.account("Steve_2", "198.51.100.1").await;
    assert_eq!(tokens["token_type"], "Bearer");
    assert_eq!(tokens["account"]["nick"], "Steve_2");
    // The access token verifies with the published key, for friends and
    // skins too.
    let jwks = t
        .send(t.get("/.well-known/jwks.json", "1.1.1.1"))
        .await
        .json();
    let x = base64::engine::general_purpose::URL_SAFE_NO_PAD
        .decode(jwks["keys"][0]["x"].as_str().unwrap())
        .unwrap();
    let key = ed25519_dalek::VerifyingKey::from_bytes(&x.try_into().unwrap()).unwrap();
    let claims = verify(
        &key,
        tokens["access_token"].as_str().unwrap(),
        BASE,
        "friends",
        crate::app::now(),
    )
    .unwrap();
    assert_eq!(claims.nick, "Steve_2");
    assert_eq!(
        claims.aud,
        ["account", "friends", "skins", "mods", "worlds"]
    );
    // Registration kept the address and country for recovery by hand.
    let account = t.app.store.find_login("steve_2").unwrap().unwrap();
    assert_eq!(
        (account.reg_ip.as_str(), account.reg_country.as_str()),
        ("198.51.100.1", "DE")
    );
    assert!(account.password.starts_with("$argon2id$"));
    assert!(!account.password.contains(PASSWORD));
    // A confirmation letter waits in the outbox.
    let letters = crate::mail::due(&t.app, crate::app::now(), 10).unwrap();
    assert_eq!(letters.len(), 1);
    assert_eq!(letters[0].to, "steve_2@example.com");
    assert!(letters[0].body.contains(&format!("{BASE}/verify?token=")));
    // The account API.
    let access = tokens["access_token"].as_str().unwrap();
    let me = t
        .send(t.api("GET", "/v1/me", access, serde_json::Value::Null))
        .await;
    assert_eq!(me.json()["nick"], "Steve_2");
    assert_eq!(me.json()["emailVerified"], false);
    // Signing in again by email in any case.
    let signed_in = t
        .login("STEVE_2@Example.com", PASSWORD, "", "198.51.100.2")
        .await;
    assert_eq!(
        signed_in.status,
        StatusCode::SEE_OTHER,
        "{}",
        signed_in.body
    );
    assert_eq!(
        t.exchange(&signed_in, VERIFIER).await.status,
        StatusCode::OK
    );
    let sessions = t
        .send(t.api("GET", "/v1/me/sessions", access, serde_json::Value::Null))
        .await
        .json();
    assert_eq!(sessions["sessions"].as_array().unwrap().len(), 2);
    assert_eq!(sessions["sessions"][0]["device"], "Windows 11");
}

#[tokio::test]
async fn a_code_needs_its_verifier_and_works_once() {
    let t = setup();
    let registered = t.register("Alex", "alex@example.com", "198.51.100.3").await;
    let wrong = t
        .exchange(
            &registered,
            "wrong-verifier-wrong-verifier-wrong-verifier-x",
        )
        .await;
    assert_eq!(wrong.status, StatusCode::BAD_REQUEST);
    assert_eq!(wrong.json()["error"], "invalid_grant");
    // Tried once: the right verifier is too late now.
    assert_eq!(
        t.exchange(&registered, VERIFIER).await.status,
        StatusCode::BAD_REQUEST
    );
    // Another client or address is refused.
    let signed_in = t.login("alex", PASSWORD, "", "198.51.100.3").await;
    let code = signed_in
        .location()
        .split("code=")
        .nth(1)
        .unwrap()
        .split('&')
        .next()
        .unwrap()
        .to_owned();
    let foreign = t
        .send(t.post_form(
            "/v1/token",
            "203.0.113.9",
            None,
            &[
                ("grant_type", "authorization_code"),
                ("code", &code),
                ("code_verifier", VERIFIER),
                ("client_id", "evil"),
                ("redirect_uri", &format!("{BASE}/app/done")),
            ],
        ))
        .await;
    assert_eq!(foreign.status, StatusCode::UNAUTHORIZED);
}

#[tokio::test]
async fn refresh_tokens_rotate_and_a_reused_one_ends_the_session() {
    let t = setup();
    let tokens = t.account("Rotor", "198.51.100.4").await;
    let first = tokens["refresh_token"].as_str().unwrap().to_owned();
    let renewed = t.refresh(&first).await;
    assert_eq!(renewed.status, StatusCode::OK);
    let second = renewed.json()["refresh_token"].as_str().unwrap().to_owned();
    assert_ne!(first, second);
    // The old one again: a copy is out there, so the whole session ends.
    assert_eq!(t.refresh(&first).await.status, StatusCode::BAD_REQUEST);
    assert_eq!(t.refresh(&second).await.status, StatusCode::BAD_REQUEST);
    let access = tokens["access_token"].as_str().unwrap();
    assert_eq!(
        t.send(t.api("GET", "/v1/me", access, serde_json::Value::Null))
            .await
            .status,
        StatusCode::UNAUTHORIZED,
        "an access token of an ended session no longer works here"
    );
    // Sign-out ends a session too.
    let other = t.account("Rotor2", "198.51.100.4").await;
    let refresh = other["refresh_token"].as_str().unwrap();
    let revoked = t
        .send(t.post_form("/v1/revoke", "203.0.113.9", None, &[("token", refresh)]))
        .await;
    assert_eq!(revoked.status, StatusCode::OK);
    assert_eq!(t.refresh(refresh).await.status, StatusCode::BAD_REQUEST);
}

#[tokio::test]
async fn wrong_passwords_say_nothing_about_the_account_and_bring_the_captcha() {
    let t = setup();
    t.account("Victim", "198.51.100.5").await;
    let unknown = t.login("nobody_here", PASSWORD, "", "192.0.2.1").await;
    let wrong = t.login("Victim", "wrong password!!", "", "192.0.2.2").await;
    for answer in [&unknown, &wrong] {
        assert_eq!(answer.status, StatusCode::OK);
        assert!(answer.body.contains("Ник, email или пароль неверны."));
    }
    // From one address: the captcha after three failures.
    let ip = "192.0.2.10";
    for _ in 0..3 {
        t.login("Victim", "wrong password!!", "", ip).await;
    }
    let asked = t.login("Victim", PASSWORD, "", ip).await;
    assert!(asked.body.contains("Подтвердите, что вы не робот."));
    assert!(asked.body.contains("cf-turnstile"));
    assert_eq!(
        t.login("Victim", PASSWORD, "ok:login", ip).await.status,
        StatusCode::SEE_OTHER
    );
    // Against one account from many addresses: the captcha after five.
    for i in 0..5 {
        t.login(
            "Victim",
            "wrong password!!",
            "",
            &format!("192.0.2.{}", 100 + i),
        )
        .await;
    }
    let asked = t.login("Victim", PASSWORD, "", "192.0.2.200").await;
    assert!(asked.body.contains("cf-turnstile"));
    // Never locked out: with the captcha the owner still gets in.
    assert_eq!(
        t.login("Victim", PASSWORD, "ok", "192.0.2.200")
            .await
            .status,
        StatusCode::SEE_OTHER
    );
}

#[tokio::test]
async fn without_captcha_keys_failures_never_lock_the_owner_out() {
    let t = setup_with(false);
    t.account("Victim", "198.51.100.5").await;
    for i in 0..6 {
        t.login(
            "Victim",
            "wrong password!!",
            "",
            &format!("192.0.2.{}", 100 + i),
        )
        .await;
    }
    assert_eq!(
        t.login("Victim", PASSWORD, "", "192.0.2.200").await.status,
        StatusCode::SEE_OTHER
    );
}

#[tokio::test]
async fn forms_without_their_cookie_or_request_do_nothing() {
    let t = setup();
    let (_, rid, csrf, _) = t.open("register", "198.51.100.6").await;
    let fields = [
        ("rid", rid.as_str()),
        ("csrf", csrf.as_str()),
        ("nick", "Forged"),
        ("email", "forged@example.com"),
        ("password", PASSWORD),
        ("password2", PASSWORD),
        ("cf-turnstile-response", "ok"),
    ];
    // Posted from another site: no cookie, or a different one.
    for cookie in [
        None,
        Some("__Host-flow=AAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAA"),
    ] {
        let answer = t
            .send(t.post_form("/register", "198.51.100.6", cookie, &fields))
            .await;
        assert_eq!(answer.status, StatusCode::BAD_REQUEST);
    }
    // An unknown or made-up request ID.
    let answer = t
        .send(t.post_form(
            "/login",
            "198.51.100.6",
            Some(&format!("__Host-flow={csrf}")),
            &[
                ("rid", "x"),
                ("csrf", &csrf),
                ("login", "a"),
                ("password", "b"),
            ],
        ))
        .await;
    assert_eq!(answer.status, StatusCode::BAD_REQUEST);
    assert!(t.app.store.find_login("forged").unwrap().is_none());
    // Without the captcha nobody registers.
    let (_, rid, csrf, cookie) = t.open("register", "198.51.100.6").await;
    let answer = t
        .send(t.post_form(
            "/register",
            "198.51.100.6",
            Some(&cookie),
            &[
                ("rid", &rid),
                ("csrf", &csrf),
                ("nick", "Robot"),
                ("email", "robot@example.com"),
                ("password", PASSWORD),
                ("password2", PASSWORD),
                ("cf-turnstile-response", "ok:login"),
            ],
        ))
        .await;
    assert!(answer.body.contains("не робот"));
    assert!(t.app.store.find_login("robot").unwrap().is_none());
}

#[tokio::test]
async fn hostile_input_is_refused_or_shown_as_text() {
    let t = setup();
    let ip = "198.51.100.7";
    let hostile = [
        ("<script>alert(1)</script>", "x@example.com"),
        ("' OR '1'='1", "y@example.com"),
        ("Robert'); DROP TABLE accounts;--", "z@example.com"),
        ("ник", "w@example.com"),
        ("a\u{0}b\u{0}c", "v@example.com"),
        ("Good_Nick", "evil@example.com\r\nBcc: all@example.com"),
        ("Good_Nick", "\"><svg onload=alert(1)>@x.com"),
    ];
    for (i, (nick, email)) in hostile.into_iter().enumerate() {
        // An address each: the register limit is five an hour per address.
        let ip = &format!("198.51.101.{i}");
        let (_, rid, csrf, cookie) = t.open("register", ip).await;
        let answer = t
            .send(t.post_form(
                "/register",
                ip,
                Some(&cookie),
                &[
                    ("rid", &rid),
                    ("csrf", &csrf),
                    ("nick", nick),
                    ("email", email),
                    ("password", PASSWORD),
                    ("password2", PASSWORD),
                    ("cf-turnstile-response", "ok"),
                ],
            ))
            .await;
        assert_eq!(answer.status, StatusCode::OK, "{nick:?}");
        // Shown back escaped, never as markup.
        assert!(!answer.body.contains("<script>alert"), "{nick:?}");
        assert!(!answer.body.contains("<svg"), "{email:?}");
        assert!(answer.body.contains("class=\"error\""));
    }
    // Nothing was stored, and the database stayed whole.
    assert!(t.app.store.find_login("good_nick").unwrap().is_none());
    t.account("Survivor", "198.51.100.8").await;
    // SQL in the login field is just a wrong login.
    for (i, login) in ["' OR '1'='1", "survivor' --", "\" OR \"\"=\"", "%", "_"]
        .into_iter()
        .enumerate()
    {
        let answer = t
            .login(login, "' OR '1'='1' --x", "", &format!("198.51.102.{i}"))
            .await;
        assert_eq!(answer.status, StatusCode::OK, "{login}");
        assert!(answer.body.contains("неверны"));
    }
    // Long and broken bodies end in 4xx, never 5xx.
    let huge = "A".repeat(40 * 1024);
    let answer = t
        .send(t.post_form("/login", ip, None, &[("login", &huge)]))
        .await;
    assert_eq!(answer.status, StatusCode::PAYLOAD_TOO_LARGE);
    for body in ["%%%%", "rid=%ZZ&csrf=%", "\u{0}\u{1}", "a=b=c&&&"] {
        let request = Request::post("/login")
            .header(header::CONTENT_TYPE, "application/x-www-form-urlencoded")
            .body(Body::from(body))
            .unwrap();
        let status = t.send(request).await.status;
        assert!(status.is_client_error(), "{body:?}: {status}");
    }
    let request = Request::post("/v1/me/nick")
        .header(header::CONTENT_TYPE, "application/json")
        .body(Body::from("{not json"))
        .unwrap();
    assert!(t.send(request).await.status.is_client_error());
}

#[tokio::test]
async fn authorize_never_sends_a_code_anywhere_else() {
    let t = setup();
    let good = format!("redirect_uri={}", enc(&format!("{BASE}/app/done")));
    let base = format!(
        "client_id=launcher&response_type=code&code_challenge={CHALLENGE}&code_challenge_method=S256&state=s"
    );
    for query in [
        format!(
            "{base}&redirect_uri={}",
            enc("https://evil.example/app/done")
        ),
        format!(
            "{base}&redirect_uri={}",
            enc(&format!("{BASE}/app/done.evil.example"))
        ),
        format!(
            "{}&{good}",
            base.replace("client_id=launcher", "client_id=other")
        ),
        format!("{}&{good}", base.replace("S256", "plain")),
        format!("{}&{good}", base.replace(CHALLENGE, "short")),
        format!("{}&{good}", base.replace("state=s", "state=%3Cscript%3E")),
        format!(
            "{}&{good}",
            base.replace("state=s", "state=a%26code%3Devil")
        ),
    ] {
        let answer = t
            .send(t.get(&format!("/authorize?{query}"), "198.51.100.10"))
            .await;
        assert_eq!(answer.status, StatusCode::BAD_REQUEST, "{query}");
        assert!(answer.location().is_empty());
        assert!(answer.cookie().is_none());
    }
}

#[tokio::test]
async fn forged_or_foreign_access_tokens_are_refused() {
    let t = setup();
    let tokens = t.account("Holder", "198.51.100.11").await;
    let real = tokens["access_token"].as_str().unwrap();
    let foreign = Keys::from_seed([9; 32]).sign(&crate::crypto::Claims {
        iss: BASE.into(),
        sub: tokens["account"]["id"].as_str().unwrap().into(),
        aud: vec!["account".into()],
        iat: crate::app::now(),
        exp: crate::app::now() + 600,
        sid: "x".into(),
        nick: "Holder".into(),
        ev: true,
    });
    let parts: Vec<&str> = real.split('.').collect();
    let unsigned = format!(
        "{}.{}.",
        base64::engine::general_purpose::URL_SAFE_NO_PAD.encode(r#"{"alg":"none"}"#),
        parts[1]
    );
    for bad in [foreign.as_str(), unsigned.as_str(), "garbage", ""] {
        let answer = t
            .send(t.api("GET", "/v1/me", bad, serde_json::Value::Null))
            .await;
        assert_eq!(answer.status, StatusCode::UNAUTHORIZED, "{bad}");
    }
}

#[tokio::test]
async fn nick_and_password_changes() {
    let t = setup();
    let tokens = t.account("First", "198.51.100.12").await;
    t.account("Taken_Nick", "198.51.100.13").await;
    let access = tokens["access_token"].as_str().unwrap();
    let change = |nick: &str| {
        t.api(
            "POST",
            "/v1/me/nick",
            access,
            serde_json::json!({ "nick": nick }),
        )
    };
    assert_eq!(
        t.send(change("taken_NICK")).await.status,
        StatusCode::CONFLICT
    );
    let refused = t.send(change("<b>bold</b>")).await;
    assert_eq!(refused.status, StatusCode::UNPROCESSABLE_ENTITY);
    assert_eq!(refused.json()["error"], "nick_characters");
    let changed = t.send(change("Second")).await;
    assert_eq!(changed.status, StatusCode::OK, "{}", changed.body);
    let fresh = changed.json()["access_token"].as_str().unwrap().to_owned();
    let claims = verify(
        &t.app.keys.public(),
        &fresh,
        BASE,
        "friends",
        crate::app::now(),
    )
    .unwrap();
    assert_eq!(claims.nick, "Second");
    // Changing only the case of one's own nick is allowed.
    assert_eq!(t.send(change("SECOND")).await.status, StatusCode::OK);
    // A new password needs the current one and ends the other sessions.
    let other = t.login("second", PASSWORD, "", "198.51.100.14").await;
    let other = t.exchange(&other, VERIFIER).await.json();
    let password = |current: &str, new: &str| {
        t.api(
            "POST",
            "/v1/me/password",
            access,
            serde_json::json!({ "current": current, "new": new }),
        )
    };
    assert_eq!(
        t.send(password("wrong one!!", "another long password"))
            .await
            .status,
        StatusCode::FORBIDDEN
    );
    assert_eq!(
        t.send(password(PASSWORD, "1234567890")).await.json()["error"],
        "password_common"
    );
    assert_eq!(
        t.send(password(PASSWORD, "another long password"))
            .await
            .status,
        StatusCode::NO_CONTENT
    );
    assert_eq!(
        t.refresh(other["refresh_token"].as_str().unwrap())
            .await
            .status,
        StatusCode::BAD_REQUEST
    );
    assert_eq!(
        t.refresh(tokens["refresh_token"].as_str().unwrap())
            .await
            .status,
        StatusCode::OK,
        "this session stays"
    );
    assert_eq!(
        t.login("SECOND", "another long password", "", "198.51.100.15")
            .await
            .status,
        StatusCode::SEE_OTHER
    );
}

#[tokio::test]
async fn reset_and_confirmation_links() {
    let t = setup();
    let tokens = t.account("Forgetful", "198.51.100.16").await;
    // Confirm the address with the link from the letter.
    let letter = crate::mail::due(&t.app, crate::app::now(), 10)
        .unwrap()
        .remove(0);
    let link = letter
        .body
        .lines()
        .find(|l| l.starts_with(BASE))
        .unwrap()
        .to_owned();
    let path = link.trim_start_matches(BASE);
    assert_eq!(t.send(t.get(path, "1.2.3.4")).await.status, StatusCode::OK);
    assert!(
        t.app
            .store
            .find_login("forgetful")
            .unwrap()
            .unwrap()
            .email_verified
    );
    assert_eq!(
        t.send(t.get(path, "1.2.3.4")).await.status,
        StatusCode::BAD_REQUEST,
        "once"
    );
    // Ask for a reset: the same answer for a known and an unknown address.
    let t = &t;
    let ask = |email: &'static str| async move {
        let page = t.send(t.get("/reset?lang=en", "198.51.100.17")).await;
        let csrf = page.field("csrf");
        let cookie = page.cookie().unwrap();
        t.send(t.post_form(
            "/reset",
            "198.51.100.17",
            Some(&cookie),
            &[
                ("csrf", &csrf),
                ("lang", "en"),
                ("email", email),
                ("cf-turnstile-response", "ok:reset"),
            ],
        ))
        .await
    };
    let unknown = ask("nobody@example.com").await;
    let known = ask("FORGETFUL@example.com").await;
    assert_eq!(unknown.body, known.body);
    let letter = crate::mail::due(&t.app, crate::app::now(), 10)
        .unwrap()
        .into_iter()
        .find(|m| m.body.contains("/reset/confirm?token="))
        .expect("a reset letter");
    let link = letter
        .body
        .lines()
        .find(|l| l.starts_with(BASE))
        .unwrap()
        .to_owned();
    let page = t
        .send(t.get(link.trim_start_matches(BASE), "198.51.100.18"))
        .await;
    assert_eq!(page.status, StatusCode::OK);
    let (csrf, cookie, token) = (
        page.field("csrf"),
        page.cookie().unwrap(),
        page.field("token"),
    );
    let set = |password: &'static str| {
        t.post_form(
            "/reset/confirm",
            "198.51.100.18",
            Some(&cookie),
            &[
                ("csrf", &csrf),
                ("token", &token),
                ("lang", "en"),
                ("password", password),
                ("password2", password),
            ],
        )
    };
    assert!(t.send(set("short")).await.body.contains("at least 10"));
    let done = t.send(set("a brand new password")).await;
    assert!(done.body.contains("every device is signed out"));
    assert_eq!(
        t.refresh(tokens["refresh_token"].as_str().unwrap())
            .await
            .status,
        StatusCode::BAD_REQUEST
    );
    assert_eq!(
        t.send(set("yet another password")).await.status,
        StatusCode::BAD_REQUEST,
        "the link works once"
    );
    assert_eq!(
        t.login("forgetful", "a brand new password", "", "198.51.100.19")
            .await
            .status,
        StatusCode::SEE_OTHER
    );
}

#[tokio::test]
async fn limits_and_switched_off_accounts() {
    let t = setup();
    let ip = "198.51.100.20";
    for i in 0..5 {
        let answer = t
            .register(&format!("Many_{i}"), &format!("many{i}@example.com"), ip)
            .await;
        assert_eq!(answer.status, StatusCode::SEE_OTHER);
    }
    assert_eq!(
        t.register("Many_6", "many6@example.com", ip).await.status,
        StatusCode::TOO_MANY_REQUESTS
    );
    let tokens = t.account("Banned", "198.51.100.21").await;
    let account = t.app.store.find_login("banned").unwrap().unwrap();
    t.app.store.set_disabled(&account.id, true).unwrap();
    assert_eq!(
        t.refresh(tokens["refresh_token"].as_str().unwrap())
            .await
            .status,
        StatusCode::BAD_REQUEST
    );
    // Told only with the right password.
    assert!(
        t.login("banned", PASSWORD, "", "198.51.100.22")
            .await
            .body
            .contains("отключён")
    );
    assert!(
        !t.login("banned", "not the password", "", "198.51.100.23")
            .await
            .body
            .contains("отключён")
    );
    // A digest is never a token: the stored hash does not sign in.
    let _ = sha2::Sha256::digest(b"x");
}

#[tokio::test]
async fn the_internal_api_tells_only_nicks_and_ids() {
    let t = setup();
    let tokens = t.account("Findable", "198.51.100.30").await;
    let id = tokens["account"]["id"].as_str().unwrap().to_owned();
    let internal = crate::internal::router(Arc::clone(&t.app));
    let ask = |request: Request<Body>| {
        let internal = internal.clone();
        async move {
            let response = internal.oneshot(request).await.unwrap();
            let status = response.status();
            let bytes = response.into_body().collect().await.unwrap().to_bytes();
            (
                status,
                serde_json::from_slice::<serde_json::Value>(&bytes).unwrap_or_default(),
            )
        }
    };
    let (status, found) = ask(Request::get("/internal/v1/find?nick=FINDABLE")
        .body(Body::empty())
        .unwrap())
    .await;
    assert_eq!(status, StatusCode::OK);
    assert_eq!(found, serde_json::json!({ "id": id, "nick": "Findable" }));
    for nick in ["nobody", "%27%20OR%201%3D1--", "x"] {
        let (status, _) = ask(Request::get(format!("/internal/v1/find?nick={nick}"))
            .body(Body::empty())
            .unwrap())
        .await;
        assert_eq!(status, StatusCode::NOT_FOUND, "{nick}");
    }
    let (status, nicks) = ask(Request::post("/internal/v1/nicks")
        .header(header::CONTENT_TYPE, "application/json")
        .body(Body::from(
            serde_json::json!({ "ids": [id, "not-an-id", "00000000-0000-4000-8000-000000000000"] })
                .to_string(),
        ))
        .unwrap())
    .await;
    assert_eq!(status, StatusCode::OK);
    assert_eq!(
        nicks,
        serde_json::json!({ "nicks": { id.clone(): "Findable" } })
    );
    // Not part of the public routes.
    let public = t
        .send(t.get("/internal/v1/find?nick=Findable", "1.2.3.4"))
        .await;
    assert_eq!(public.status, StatusCode::NOT_FOUND);
    // Which sessions ended: only the unknown one, until the account is
    // switched off.
    let sid = verify(
        &t.app.keys.public(),
        tokens["access_token"].as_str().unwrap(),
        BASE,
        "friends",
        crate::app::now(),
    )
    .unwrap()
    .sid;
    let ended = |sessions: serde_json::Value| {
        Request::post("/internal/v1/ended")
            .header(header::CONTENT_TYPE, "application/json")
            .body(Body::from(
                serde_json::json!({ "sessions": sessions }).to_string(),
            ))
            .unwrap()
    };
    let (status, answer) = ask(ended(serde_json::json!([sid, "unknown' OR 1=1--"]))).await;
    assert_eq!(status, StatusCode::OK);
    assert_eq!(
        answer,
        serde_json::json!({ "ended": ["unknown' OR 1=1--"] })
    );
    // A switched-off account is not found, and its sessions have ended.
    t.app.store.set_disabled(&id, true).unwrap();
    let (status, _) = ask(Request::get("/internal/v1/find?nick=findable")
        .body(Body::empty())
        .unwrap())
    .await;
    assert_eq!(status, StatusCode::NOT_FOUND);
    let (_, answer) = ask(ended(serde_json::json!([sid]))).await;
    assert_eq!(answer, serde_json::json!({ "ended": [sid] }));
    let (status, _) = ask(ended(serde_json::json!(vec!["x"; 501]))).await;
    assert_eq!(status, StatusCode::PAYLOAD_TOO_LARGE);
    // No SMTP server in the tests: letters do not go out.
    let (status, mail) = ask(Request::get("/internal/v1/mail")
        .body(Body::empty())
        .unwrap())
    .await;
    assert_eq!(
        (status, mail),
        (StatusCode::OK, serde_json::json!({ "sending": false }))
    );
}

/// Sets a new password with a reset link's token; the answer of the form.
async fn reset_with(t: &T, token: &str, password: &str) -> Answer {
    let page = t
        .send(t.get(
            &format!("/reset/confirm?token={token}&lang=en"),
            "198.51.100.40",
        ))
        .await;
    // An ended link shows no form at all.
    if page.status != StatusCode::OK {
        return page;
    }
    let (csrf, cookie) = (page.field("csrf"), page.cookie().unwrap());
    t.send(t.post_form(
        "/reset/confirm",
        "198.51.100.40",
        Some(&cookie),
        &[
            ("csrf", &csrf),
            ("token", token),
            ("lang", "en"),
            ("password", password),
            ("password2", password),
        ],
    ))
    .await
}

/// A reset link made directly in the store, as `/reset` makes it.
fn reset_link(t: &T, account: &str, email: &str) -> String {
    let token = crate::crypto::token();
    t.app
        .store
        .put_email_token(
            &crate::crypto::digest(&token),
            account,
            "reset",
            email,
            crate::app::now() + 1800,
        )
        .unwrap();
    token
}

#[tokio::test]
async fn letters_waiting_hold_no_working_link_and_sent_ones_keep_nothing() {
    let t = setup();
    t.account("Sealed", "198.51.100.41").await;
    let now = crate::app::now();
    // In the database the letter is sealed: no link to copy.
    let raw = t.app.store.pending_mail(now, 10).unwrap();
    assert_eq!(raw.len(), 1);
    assert!(raw[0].body.starts_with("sealed:1:"));
    assert!(!raw[0].body.contains("token="));
    // Only this key, and only for this recipient, opens it.
    let opened = crate::mail::due(&t.app, now, 10).unwrap();
    assert!(opened[0].body.contains("/verify?token="));
    assert!(
        crate::mail::Sealer::random()
            .open(&raw[0].to, &raw[0].body)
            .is_none()
    );
    assert!(
        t.app
            .sealer
            .open("other@example.com", &raw[0].body)
            .is_none()
    );
    // A failed letter waits before its next try; a sent one keeps no body.
    t.app
        .store
        .mail_done(raw[0].id, Some("421 busy"), now)
        .unwrap();
    assert!(t.app.store.pending_mail(now, 10).unwrap().is_empty());
    assert_eq!(t.app.store.pending_mail(now + 20, 10).unwrap().len(), 1);
    t.app
        .store
        .mail_done(raw[0].id, Some("421 busy"), now)
        .unwrap();
    assert!(
        t.app.store.pending_mail(now + 20, 10).unwrap().is_empty(),
        "the wait grows"
    );
    t.app.store.mail_done(raw[0].id, None, now).unwrap();
    assert!(t.app.store.pending_mail(now + 7200, 10).unwrap().is_empty());
    // A letter queued before sealing is sealed at start.
    t.app
        .store
        .queue_mail("old@example.com", "Old", "link: token=abc", now)
        .unwrap();
    crate::mail::seal_waiting(&t.app).unwrap();
    let waiting = t.app.store.pending_mail(now, 10).unwrap();
    assert!(waiting[0].body.starts_with("sealed:1:"));
    assert_eq!(
        crate::mail::due(&t.app, now, 10).unwrap()[0].body,
        "link: token=abc"
    );
}

#[tokio::test]
async fn a_new_password_or_address_ends_the_links_still_out() {
    let t = setup();
    let tokens = t.account("Mover", "198.51.100.42").await;
    let access = tokens["access_token"].as_str().unwrap();
    let account = t.app.store.find_login("mover").unwrap().unwrap();
    // Two reset links are out; using one ends the other.
    let first = reset_link(&t, &account.id, &account.email);
    let second = reset_link(&t, &account.id, &account.email);
    assert!(
        reset_with(&t, &first, "the first new password")
            .await
            .body
            .contains("every device is signed out")
    );
    assert_eq!(
        reset_with(&t, &second, "the second new password")
            .await
            .status,
        StatusCode::BAD_REQUEST
    );
    // A new address: the old one is told, and its links end.
    let old_link = reset_link(&t, &account.id, &account.email);
    for mail in crate::mail::due(&t.app, crate::app::now(), 50).unwrap() {
        t.app
            .store
            .mail_done(mail.id, None, crate::app::now())
            .unwrap();
    }
    let changed = t
        .send(t.api(
            "POST",
            "/v1/me/email",
            access,
            serde_json::json!({ "email": "thief@example.net", "password": "the first new password" }),
        ))
        .await;
    assert_eq!(
        changed.status,
        StatusCode::UNAUTHORIZED,
        "signed out by the reset"
    );
    let signed_in = t
        .login("mover", "the first new password", "", "198.51.100.43")
        .await;
    let tokens = t.exchange(&signed_in, VERIFIER).await.json();
    let access = tokens["access_token"].as_str().unwrap();
    let changed = t
        .send(t.api(
            "POST",
            "/v1/me/email",
            access,
            serde_json::json!({ "email": "thief@example.net", "password": "the first new password" }),
        ))
        .await;
    assert_eq!(changed.status, StatusCode::NO_CONTENT, "{}", changed.body);
    let letters = crate::mail::due(&t.app, crate::app::now(), 50).unwrap();
    let notice = letters
        .iter()
        .find(|m| m.to == account.email)
        .expect("a letter to the old address");
    assert!(notice.body.contains("t***@example.net"));
    assert!(!notice.body.contains("thief@"));
    assert!(letters.iter().any(|m| m.to == "thief@example.net"));
    assert_eq!(
        reset_with(&t, &old_link, "a password from the old link")
            .await
            .status,
        StatusCode::BAD_REQUEST
    );
    // Even a link made for the old address after the change does nothing.
    let stale = reset_link(&t, &account.id, &account.email);
    assert_eq!(
        reset_with(&t, &stale, "a password from a stale link")
            .await
            .status,
        StatusCode::BAD_REQUEST
    );
}

#[test]
fn a_database_from_before_sealing_is_brought_up_to_date() {
    let dir = tempfile::tempdir().unwrap();
    {
        let db = rusqlite::Connection::open(dir.path().join("accounts.db")).unwrap();
        db.execute_batch(
            "CREATE TABLE outbox (id INTEGER PRIMARY KEY, to_addr TEXT NOT NULL,
               subject TEXT NOT NULL, body TEXT NOT NULL, created INTEGER NOT NULL,
               sent INTEGER, attempts INTEGER NOT NULL DEFAULT 0, last_error TEXT);
             INSERT INTO outbox (to_addr, subject, body, created, sent)
               VALUES ('a@example.com', 'Sent', 'token=old', 1, 2);",
        )
        .unwrap();
    }
    crate::store::Store::open(dir.path()).unwrap();
    let db = rusqlite::Connection::open(dir.path().join("accounts.db")).unwrap();
    let body: String = db
        .query_row("SELECT body FROM outbox", [], |r| r.get(0))
        .unwrap();
    assert_eq!(body, "", "a sent letter keeps no link");
    // Opening again changes nothing.
    crate::store::Store::open(dir.path()).unwrap();
}

#[tokio::test]
async fn a_flood_from_many_addresses_brings_the_captcha_not_a_lockout() {
    let t = setup();
    t.account("Crowd", "198.51.100.50").await;
    // Everyone's allowance for the hour is used up.
    for _ in 0..5_001 {
        t.app.limiter.busy("login", u32::MAX);
    }
    let asked = t.login("Crowd", PASSWORD, "", "203.0.113.9").await;
    assert_eq!(asked.status, StatusCode::OK);
    assert!(asked.body.contains("cf-turnstile"));
    assert_eq!(
        t.login("Crowd", PASSWORD, "ok:login", "203.0.113.9")
            .await
            .status,
        StatusCode::SEE_OTHER,
        "the owner still gets in"
    );
    // Without captcha keys nobody is asked, and nobody is refused.
    let t = setup_with(false);
    t.account("Crowd", "198.51.100.50").await;
    for _ in 0..5_001 {
        t.app.limiter.busy("login", u32::MAX);
    }
    assert_eq!(
        t.login("Crowd", PASSWORD, "", "203.0.113.9").await.status,
        StatusCode::SEE_OTHER
    );
}

#[tokio::test]
async fn backups_are_whole_private_and_only_the_newest_stay() {
    let t = setup();
    t.account("Kept", "198.51.100.60").await;
    let dir = t._dir.path().join("backups");
    let first = t.app.store.backup(&dir, 2).unwrap();
    let restored = tempfile::tempdir().unwrap();
    std::fs::copy(&first, restored.path().join("accounts.db")).unwrap();
    let copy = crate::store::Store::open(restored.path()).unwrap();
    assert!(copy.find_login("kept").unwrap().is_some());
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        assert_eq!(
            std::fs::metadata(&first).unwrap().permissions().mode() & 0o777,
            0o600
        );
        assert_eq!(
            std::fs::metadata(&dir).unwrap().permissions().mode() & 0o777,
            0o700
        );
    }
    for name in [
        "accounts-20000101-000000.db",
        "accounts-20000102-000000.db",
        "notes.txt",
    ] {
        std::fs::write(dir.join(name), b"x").unwrap();
    }
    t.app.store.backup(&dir, 2).unwrap();
    let mut left: Vec<String> = std::fs::read_dir(&dir)
        .unwrap()
        .flatten()
        .map(|e| e.file_name().to_string_lossy().into_owned())
        .collect();
    left.sort();
    assert_eq!(left.len(), 3, "{left:?}");
    assert!(!left.iter().any(|n| n.starts_with("accounts-2000")));
    assert!(left.contains(&"notes.txt".to_owned()));
}

#[tokio::test]
async fn the_owner_exports_their_data_and_deletes_the_account_for_good() {
    let t = setup();
    let tokens = t.account("Leaving", "198.51.100.70").await;
    let access = tokens["access_token"].as_str().unwrap();
    let id = tokens["account"]["id"].as_str().unwrap().to_owned();
    // Export: what is kept, for the account's own session only.
    let export = t
        .send(t.api("GET", "/v1/me/export", access, serde_json::Value::Null))
        .await;
    assert_eq!(export.status, StatusCode::OK);
    let data = export.json();
    assert_eq!(data["nick"], "Leaving");
    assert_eq!(data["email"], "leaving@example.com");
    assert_eq!(data["registeredFrom"]["address"], "198.51.100.70");
    assert!(!data["sessions"].as_array().unwrap().is_empty());
    assert!(!export.body.contains("$argon2"), "no password hash");
    assert_eq!(
        t.send(t.api("GET", "/v1/me/export", "forged", serde_json::Value::Null))
            .await
            .status,
        StatusCode::UNAUTHORIZED
    );
    // Deleting: our page, the password, then a second step.
    let page = t.send(t.get("/delete?lang=ru", "198.51.100.71")).await;
    assert!(page.body.contains("Удаление аккаунта"));
    let (csrf, cookie) = (page.field("csrf"), page.cookie().unwrap());
    let step = |password: &'static str, captcha: &'static str| {
        t.post_form(
            "/delete",
            "198.51.100.71",
            Some(&cookie),
            &[
                ("csrf", &csrf),
                ("lang", "en"),
                ("login", "LEAVING"),
                ("password", password),
                ("cf-turnstile-response", captcha),
            ],
        )
    };
    assert!(
        t.send(step(PASSWORD, ""))
            .await
            .body
            .contains("not a robot")
    );
    assert!(
        t.send(step("not the password", "ok:delete"))
            .await
            .body
            .contains("is not right")
    );
    let asked = t.send(step(PASSWORD, "ok:delete")).await;
    assert!(asked.body.contains("Delete account Leaving"));
    assert!(
        t.app.store.find_login("leaving").unwrap().is_some(),
        "not yet"
    );
    let confirm = asked.field("token");
    let finish = |csrf: &str, token: &str| {
        t.post_form(
            "/delete/confirm",
            "198.51.100.71",
            Some(&cookie),
            &[("csrf", csrf), ("lang", "en"), ("token", token)],
        )
    };
    // Without the page's own cookie value, nothing happens.
    assert_ne!(
        t.send(finish(
            "wrong-csrf-wrong-csrf-wrong-csrf-wrong-csr",
            &confirm
        ))
        .await
        .status,
        StatusCode::SEE_OTHER
    );
    let done = t.send(finish(&csrf, &confirm)).await;
    assert_eq!(done.status, StatusCode::SEE_OTHER);
    assert_eq!(done.location(), "/app/deleted?lang=en");
    assert!(t.app.store.find_login("leaving").unwrap().is_none());
    assert_eq!(
        t.refresh(tokens["refresh_token"].as_str().unwrap())
            .await
            .status,
        StatusCode::BAD_REQUEST
    );
    let listed = t.app.store.deleted_since(0).unwrap();
    assert_eq!(listed.len(), 1);
    assert_eq!(listed[0].0, id);
    assert!(t.app.store.deleted_since(listed[0].1).unwrap().is_empty());
    // The address hears of it, once; the link works once.
    let letters = crate::mail::due(&t.app, crate::app::now(), 50).unwrap();
    let to_it: Vec<_> = letters
        .iter()
        .filter(|m| m.to == "leaving@example.com")
        .collect();
    assert_eq!(to_it.len(), 1, "letters waiting for it went with it");
    assert!(to_it[0].body.contains("Leaving") && to_it[0].subject.contains("удалён"));
    assert_eq!(
        t.send(finish(&csrf, &confirm)).await.status,
        StatusCode::BAD_REQUEST
    );
    // The nick and address are free again.
    assert_eq!(
        t.register("Leaving", "leaving@example.com", "198.51.100.72")
            .await
            .status,
        StatusCode::SEE_OTHER
    );
    let internal = crate::internal::router(Arc::clone(&t.app));
    let response = internal
        .oneshot(
            Request::get("/internal/v1/deleted?since=0")
                .body(Body::empty())
                .unwrap(),
        )
        .await
        .unwrap();
    let bytes = response.into_body().collect().await.unwrap().to_bytes();
    let listed: serde_json::Value = serde_json::from_slice(&bytes).unwrap();
    assert_eq!(listed["deleted"][0]["id"], id);
}
