//! The friends API through its router, with tokens signed as the account
//! service signs them and a directory of nicks in memory.

use std::collections::BTreeMap;
use std::future::Future;
use std::pin::Pin;
use std::sync::Arc;

use axum::Router;
use axum::body::Body;
use axum::http::{Request, StatusCode, header};
use base64::Engine as _;
use base64::engine::general_purpose::URL_SAFE_NO_PAD as B64;
use ed25519_dalek::{Signer, SigningKey};
use http_body_util::BodyExt;
use serde_json::{Value, json};
use tower::ServiceExt;

use crate::api::{App, Limiter, now};
use crate::auth::Tokens;
use crate::directory::{Accounts, Directory};
use crate::hub::Hub;
use crate::store::Store;

const ISSUER: &str = "https://oauth.example";

struct People(BTreeMap<String, String>);

type Boxed<'a, T> = Pin<Box<dyn Future<Output = Result<T, String>> + Send + 'a>>;

impl Accounts for People {
    fn find<'a>(&'a self, nick: &'a str) -> Boxed<'a, Option<(String, String)>> {
        Box::pin(async move {
            Ok(self
                .0
                .iter()
                .find(|(_, n)| n.eq_ignore_ascii_case(nick))
                .map(|(id, n)| (id.clone(), n.clone())))
        })
    }

    fn nicks<'a>(&'a self, ids: &'a [String]) -> Boxed<'a, BTreeMap<String, String>> {
        Box::pin(async move {
            Ok(ids
                .iter()
                .filter_map(|id| Some((id.clone(), self.0.get(id)?.clone())))
                .collect())
        })
    }
}

fn id(n: u8) -> String {
    format!("00000000-0000-4000-8000-0000000000{n:02x}")
}

struct T {
    _dir: tempfile::TempDir,
    app: Arc<App>,
    router: Router,
    key: SigningKey,
}

fn setup() -> T {
    let dir = tempfile::tempdir().unwrap();
    let key = SigningKey::from_bytes(&[5; 32]);
    let tokens = Tokens::new(ISSUER);
    tokens.set_jwks(&json!({ "keys": [{
        "kty": "OKP", "crv": "Ed25519", "kid": "k1",
        "x": B64.encode(key.verifying_key().as_bytes()),
    }]}));
    let people = People(
        [
            (id(1), "Alice".to_owned()),
            (id(2), "Bob".to_owned()),
            (id(3), "Carol".to_owned()),
            (id(4), "Dave".to_owned()),
            (id(5), "Mallory".to_owned()),
        ]
        .into(),
    );
    let app = Arc::new(App {
        store: Store::open(dir.path()).unwrap(),
        tokens,
        directory: Directory::new(Box::new(people)),
        hub: Hub::default(),
        limiter: Limiter::default(),
        require_email: false,
    });
    T {
        _dir: dir,
        router: crate::api::router(Arc::clone(&app)),
        app,
        key,
    }
}

impl T {
    fn token_with(&self, claims: Value, kid: &str) -> String {
        let header = json!({ "alg": "EdDSA", "typ": "JWT", "kid": kid });
        let input = format!(
            "{}.{}",
            B64.encode(header.to_string()),
            B64.encode(claims.to_string())
        );
        let signature = self.key.sign(input.as_bytes());
        format!("{input}.{}", B64.encode(signature.to_bytes()))
    }

    fn token(&self, who: u8) -> String {
        let nick = ["", "Alice", "Bob", "Carol", "Dave", "Mallory"][who as usize];
        self.token_with(
            json!({
                "iss": ISSUER, "sub": id(who), "aud": ["account", "friends"],
                "iat": now(), "exp": now() + 600, "sid": "s", "nick": nick, "ev": false,
            }),
            "k1",
        )
    }

    async fn call(&self, who: u8, method: &str, uri: &str, body: Value) -> (StatusCode, Value) {
        let request = Request::builder()
            .method(method)
            .uri(uri)
            .header(header::AUTHORIZATION, format!("Bearer {}", self.token(who)))
            .header(header::CONTENT_TYPE, "application/json")
            .body(if body.is_null() {
                Body::empty()
            } else {
                Body::from(body.to_string())
            })
            .unwrap();
        self.raw(request).await
    }

    async fn raw(&self, request: Request<Body>) -> (StatusCode, Value) {
        let response = self.router.clone().oneshot(request).await.unwrap();
        let status = response.status();
        let bytes = response.into_body().collect().await.unwrap().to_bytes();
        (
            status,
            serde_json::from_slice(&bytes).unwrap_or(Value::Null),
        )
    }

    async fn state(&self, who: u8) -> Value {
        self.call(who, "GET", "/v1/state", Value::Null).await.1
    }

    async fn befriend(&self, a: u8, b: u8) {
        let nick_b = ["", "Alice", "Bob", "Carol", "Dave", "Mallory"][b as usize];
        assert_eq!(
            self.call(a, "POST", "/v1/requests", json!({ "nick": nick_b }))
                .await
                .1["result"],
            "sent"
        );
        assert_eq!(
            self.call(b, "POST", "/v1/people/accept", json!({ "id": id(a) }))
                .await
                .0,
            StatusCode::NO_CONTENT
        );
    }
}

fn nicks(list: &Value) -> Vec<String> {
    list.as_array()
        .unwrap()
        .iter()
        .map(|p| p["nick"].as_str().unwrap().to_owned())
        .collect()
}

#[tokio::test]
async fn requests_accept_decline_and_remove() {
    let t = setup();
    let (status, sent) = t
        .call(1, "POST", "/v1/requests", json!({ "nick": "bob" }))
        .await;
    assert_eq!(
        (status, sent["result"].clone()),
        (StatusCode::OK, json!("sent"))
    );
    assert_eq!(nicks(&t.state(1).await["outgoing"]), ["Bob"]);
    assert_eq!(nicks(&t.state(2).await["incoming"]), ["Alice"]);
    // Asking again changes nothing; asking back makes friends.
    t.call(1, "POST", "/v1/requests", json!({ "nick": "Bob" }))
        .await;
    let (_, back) = t
        .call(2, "POST", "/v1/requests", json!({ "nick": "Alice" }))
        .await;
    assert_eq!(back["result"], "friends");
    assert_eq!(nicks(&t.state(1).await["friends"]), ["Bob"]);
    assert_eq!(nicks(&t.state(2).await["friends"]), ["Alice"]);
    assert!(t.state(2).await["incoming"].as_array().unwrap().is_empty());
    // Declined: the request goes, the asker is not told.
    t.call(3, "POST", "/v1/requests", json!({ "nick": "Alice" }))
        .await;
    assert_eq!(
        t.call(1, "POST", "/v1/people/decline", json!({ "id": id(3) }))
            .await
            .0,
        StatusCode::NO_CONTENT
    );
    assert!(t.state(1).await["incoming"].as_array().unwrap().is_empty());
    // Cancel ours, remove a friend.
    t.call(1, "POST", "/v1/requests", json!({ "nick": "Dave" }))
        .await;
    assert_eq!(
        t.call(1, "POST", "/v1/people/cancel", json!({ "id": id(4) }))
            .await
            .0,
        StatusCode::NO_CONTENT
    );
    assert_eq!(
        t.call(2, "POST", "/v1/people/remove", json!({ "id": id(1) }))
            .await
            .0,
        StatusCode::NO_CONTENT
    );
    assert!(t.state(1).await["friends"].as_array().unwrap().is_empty());
    // Accepting nothing, unknown nicks, oneself.
    assert_eq!(
        t.call(1, "POST", "/v1/people/accept", json!({ "id": id(4) }))
            .await
            .0,
        StatusCode::NOT_FOUND
    );
    assert_eq!(
        t.call(1, "POST", "/v1/requests", json!({ "nick": "Nobody" }))
            .await
            .1["error"],
        "no_such_nick"
    );
    assert_eq!(
        t.call(1, "POST", "/v1/requests", json!({ "nick": "alice" }))
            .await
            .1["error"],
        "yourself"
    );
}

#[tokio::test]
async fn blocking_is_silent_and_complete() {
    let t = setup();
    t.befriend(1, 5).await;
    assert_eq!(
        t.call(1, "POST", "/v1/people/block", json!({ "id": id(5) }))
            .await
            .0,
        StatusCode::NO_CONTENT
    );
    // Friendship gone on both sides; Mallory is not told why.
    assert!(t.state(5).await["friends"].as_array().unwrap().is_empty());
    assert_eq!(nicks(&t.state(1).await["blocked"]), ["Mallory"]);
    assert!(t.state(5).await["blocked"].as_array().unwrap().is_empty());
    // Mallory's requests seem sent, and reach nobody.
    let (_, sent) = t
        .call(5, "POST", "/v1/requests", json!({ "nick": "Alice" }))
        .await;
    assert_eq!(sent["result"], "sent");
    assert!(t.state(1).await["incoming"].as_array().unwrap().is_empty());
    assert!(t.state(5).await["outgoing"].as_array().unwrap().is_empty());
    // Alice must unblock before asking Mallory.
    assert_eq!(
        t.call(1, "POST", "/v1/requests", json!({ "nick": "Mallory" }))
            .await
            .1["error"],
        "you_blocked"
    );
    assert_eq!(
        t.call(1, "POST", "/v1/people/unblock", json!({ "id": id(5) }))
            .await
            .0,
        StatusCode::NO_CONTENT
    );
    assert_eq!(
        t.call(1, "POST", "/v1/requests", json!({ "nick": "Mallory" }))
            .await
            .1["result"],
        "sent"
    );
}

#[tokio::test]
async fn chats_between_friends_and_rooms_of_three() {
    let t = setup();
    t.befriend(1, 2).await;
    t.befriend(1, 3).await;
    // Not friends: no room of two.
    assert_eq!(
        t.call(2, "POST", "/v1/rooms/direct", json!({ "id": id(3) }))
            .await
            .0,
        StatusCode::FORBIDDEN
    );
    let (_, opened) = t
        .call(1, "POST", "/v1/rooms/direct", json!({ "id": id(2) }))
        .await;
    let room = opened["room"].as_str().unwrap().to_owned();
    let (_, again) = t
        .call(2, "POST", "/v1/rooms/direct", json!({ "id": id(1) }))
        .await;
    assert_eq!(again["room"], room.as_str(), "one room per pair");
    let messages = format!("/v1/rooms/{room}/messages");
    let (status, sent) = t
        .call(
            1,
            "POST",
            &messages,
            json!({ "body": "  hi <b>Bob</b>\u{202E}  " }),
        )
        .await;
    assert_eq!(status, StatusCode::OK);
    assert_eq!(sent["message"]["body"], "hi <b>Bob</b>");
    let (_, rooms) = t.call(2, "GET", "/v1/rooms", Value::Null).await;
    assert_eq!(rooms["rooms"][0]["unread"], 1);
    assert_eq!(rooms["rooms"][0]["last"]["body"], "hi <b>Bob</b>");
    let (_, history) = t.call(2, "GET", &messages, Value::Null).await;
    let last = history["messages"][0]["id"].as_i64().unwrap();
    t.call(
        2,
        "POST",
        &format!("/v1/rooms/{room}/read"),
        json!({ "upto": last }),
    )
    .await;
    let (_, rooms) = t.call(2, "GET", "/v1/rooms", Value::Null).await;
    assert_eq!(rooms["rooms"][0]["unread"], 0);
    // An outsider sees nothing of it.
    assert_eq!(
        t.call(3, "GET", &messages, Value::Null).await.0,
        StatusCode::NOT_FOUND
    );
    assert_eq!(
        t.call(3, "POST", &messages, json!({ "body": "let me in" }))
            .await
            .0,
        StatusCode::NOT_FOUND
    );
    // After unfriending, the history stays and nobody writes there.
    t.call(2, "POST", "/v1/people/remove", json!({ "id": id(1) }))
        .await;
    assert_eq!(
        t.call(1, "POST", &messages, json!({ "body": "still there?" }))
            .await
            .1["error"],
        "not_friends"
    );
    assert_eq!(
        t.call(2, "GET", &messages, Value::Null).await.0,
        StatusCode::OK
    );

    // A room of Alice and two friends.
    t.befriend(1, 4).await;
    assert_eq!(
        t.call(
            1,
            "POST",
            "/v1/rooms/group",
            json!({ "name": "Duo", "members": [id(3)] })
        )
        .await
        .1["error"],
        "members",
        "2 or more friends"
    );
    assert_eq!(
        t.call(
            1,
            "POST",
            "/v1/rooms/group",
            json!({ "name": "X", "members": [id(3), id(5)] })
        )
        .await
        .1["error"],
        "not_friends"
    );
    let (_, made) = t
        .call(
            1,
            "POST",
            "/v1/rooms/group",
            json!({ "name": " Builders\n", "members": [id(3), id(4)] }),
        )
        .await;
    let group = made["room"].as_str().unwrap().to_owned();
    let (_, rooms) = t.call(3, "GET", "/v1/rooms", Value::Null).await;
    let listed = rooms["rooms"]
        .as_array()
        .unwrap()
        .iter()
        .find(|r| r["id"] == group.as_str())
        .unwrap();
    assert_eq!(listed["name"], "Builders");
    assert_eq!(nicks(&listed["members"]), ["Alice", "Carol", "Dave"]);
    let say = format!("/v1/rooms/{group}/messages");
    assert_eq!(
        t.call(4, "POST", &say, json!({ "body": "hello all" }))
            .await
            .0,
        StatusCode::OK
    );
    // Only the owner renames and removes others; anyone leaves.
    assert_eq!(
        t.call(
            3,
            "POST",
            &format!("/v1/rooms/{group}/name"),
            json!({ "name": "Mine" })
        )
        .await
        .0,
        StatusCode::FORBIDDEN
    );
    assert_eq!(
        t.call(
            3,
            "POST",
            &format!("/v1/rooms/{group}/remove"),
            json!({ "id": id(4) })
        )
        .await
        .0,
        StatusCode::FORBIDDEN
    );
    assert_eq!(
        t.call(
            1,
            "POST",
            &format!("/v1/rooms/{group}/remove"),
            json!({ "id": id(1) })
        )
        .await
        .0,
        StatusCode::NO_CONTENT,
        "the owner leaves"
    );
    let room_row = t.app.store.room(&group).unwrap().unwrap();
    assert_eq!(room_row.owner, id(3), "the next to join owns it now");
    // A blocked sender's messages are left out for whoever blocked them.
    t.call(3, "POST", "/v1/people/block", json!({ "id": id(4) }))
        .await;
    let (_, seen) = t.call(3, "GET", &say, Value::Null).await;
    assert!(seen["messages"].as_array().unwrap().is_empty());
}

#[tokio::test]
async fn bad_tokens_and_hostile_input() {
    let t = setup();
    let bad_claims = |exp: i64, aud: &str, iss: &str| json!({ "iss": iss, "sub": id(1), "aud": [aud], "iat": now(), "exp": exp, "sid": "s", "nick": "Alice", "ev": true });
    let other = SigningKey::from_bytes(&[6; 32]);
    let foreign = {
        let header = B64.encode(json!({ "alg": "EdDSA", "kid": "k1" }).to_string());
        let payload = B64.encode(bad_claims(now() + 600, "friends", ISSUER).to_string());
        let input = format!("{header}.{payload}");
        format!(
            "{input}.{}",
            B64.encode(other.sign(input.as_bytes()).to_bytes())
        )
    };
    let unsigned = format!(
        "{}.{}.",
        B64.encode(json!({ "alg": "none" }).to_string()),
        B64.encode(bad_claims(now() + 600, "friends", ISSUER).to_string())
    );
    for token in [
        t.token_with(bad_claims(now() - 1, "friends", ISSUER), "k1"),
        t.token_with(bad_claims(now() + 600, "account", ISSUER), "k1"),
        t.token_with(bad_claims(now() + 600, "friends", "https://evil"), "k1"),
        t.token_with(bad_claims(now() + 600, "friends", ISSUER), "other-kid"),
        foreign,
        unsigned,
        "garbage".to_owned(),
    ] {
        let request = Request::get("/v1/state")
            .header(header::AUTHORIZATION, format!("Bearer {token}"))
            .body(Body::empty())
            .unwrap();
        assert_eq!(t.raw(request).await.0, StatusCode::UNAUTHORIZED);
    }
    assert_eq!(
        t.raw(Request::get("/v1/state").body(Body::empty()).unwrap())
            .await
            .0,
        StatusCode::UNAUTHORIZED
    );
    // Nicks and IDs that are not nicks and IDs.
    for nick in ["' OR 1=1 --", "<script>", "a", "ник", "x\u{0}y"] {
        assert_eq!(
            t.call(1, "POST", "/v1/requests", json!({ "nick": nick }))
                .await
                .1["error"],
            "nick_invalid",
            "{nick}"
        );
    }
    for other in [
        "../../etc/passwd",
        "' OR '1'='1",
        "",
        "00000000000040008000000000000002",
    ] {
        assert_eq!(
            t.call(1, "POST", "/v1/people/block", json!({ "id": other }))
                .await
                .0,
            StatusCode::UNPROCESSABLE_ENTITY,
            "{other}"
        );
    }
    assert_eq!(
        t.call(1, "GET", "/v1/rooms/'%20OR%201=1--/messages", Value::Null)
            .await
            .0,
        StatusCode::NOT_FOUND
    );
    assert_eq!(
        t.call(1, "POST", "/v1/people/explode", json!({ "id": id(2) }))
            .await
            .0,
        StatusCode::NOT_FOUND
    );
    // Empty, oversized and broken bodies.
    t.befriend(1, 2).await;
    let (_, opened) = t
        .call(1, "POST", "/v1/rooms/direct", json!({ "id": id(2) }))
        .await;
    let messages = format!("/v1/rooms/{}/messages", opened["room"].as_str().unwrap());
    for body in [
        json!({ "body": "   " }),
        json!({ "body": "x".repeat(2001) }),
        json!({ "body": 5 }),
    ] {
        assert!(t.call(1, "POST", &messages, body).await.0.is_client_error());
    }
    let huge = Request::post(&messages)
        .header(header::AUTHORIZATION, format!("Bearer {}", t.token(1)))
        .header(header::CONTENT_TYPE, "application/json")
        .body(Body::from(format!(
            "{{\"body\":\"{}\"}}",
            "y".repeat(40_000)
        )))
        .unwrap();
    assert_eq!(t.raw(huge).await.0, StatusCode::PAYLOAD_TOO_LARGE);
    // Bursts are slowed down.
    let mut statuses = Vec::new();
    for i in 0..7 {
        statuses.push(
            t.call(1, "POST", &messages, json!({ "body": format!("m{i}") }))
                .await
                .0,
        );
    }
    assert!(statuses.contains(&StatusCode::TOO_MANY_REQUESTS));
    // A report only from someone who can see the message.
    let (_, history) = t.call(2, "GET", &messages, Value::Null).await;
    let message = history["messages"][0]["id"].as_i64().unwrap();
    assert_eq!(
        t.call(
            3,
            "POST",
            &format!("/v1/messages/{message}/report"),
            json!({ "reason": "spam" })
        )
        .await
        .0,
        StatusCode::NOT_FOUND
    );
    assert_eq!(
        t.call(
            2,
            "POST",
            &format!("/v1/messages/{message}/report"),
            json!({ "reason": "spam\u{1b}[2J" })
        )
        .await
        .0,
        StatusCode::NO_CONTENT
    );
    let reports = t.app.store.reports(10).unwrap();
    assert_eq!(reports[0]["reason"], "spam[2J");
}

#[tokio::test]
async fn live_events_reach_the_right_people() {
    let t = setup();
    t.befriend(1, 2).await;
    let (_, mut bob) = t.app.hub.join(&id(2)).unwrap();
    let (_, mut carol) = t.app.hub.join(&id(3)).unwrap();
    let (_, opened) = t
        .call(1, "POST", "/v1/rooms/direct", json!({ "id": id(2) }))
        .await;
    let room = opened["room"].as_str().unwrap();
    t.call(
        1,
        "POST",
        &format!("/v1/rooms/{room}/messages"),
        json!({ "body": "ping" }),
    )
    .await;
    match bob.try_recv().unwrap() {
        crate::hub::Event::Message { message } => assert_eq!(message.body, "ping"),
        other => panic!("{other:?}"),
    }
    assert!(carol.try_recv().is_err(), "not her room");
    // A request reaches its target as a hint to reload.
    t.call(1, "POST", "/v1/requests", json!({ "nick": "Carol" }))
        .await;
    assert_eq!(carol.try_recv().unwrap(), crate::hub::Event::Friends);
    // Five connections per account at most.
    let held: Vec<_> = (0..4).map(|_| t.app.hub.join(&id(3)).unwrap()).collect();
    assert!(t.app.hub.join(&id(3)).is_none());
    drop(held);
}

#[tokio::test]
async fn the_live_socket_needs_a_token_and_carries_messages() {
    use futures_util::StreamExt;
    use tokio_tungstenite::tungstenite::client::IntoClientRequest;
    let t = setup();
    t.befriend(1, 2).await;
    let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
    let address = listener.local_addr().unwrap();
    tokio::spawn(axum::serve(listener, t.router.clone()).into_future());
    let url = format!("ws://{address}/v1/live");
    // Without a token, or with a broken one: no socket.
    assert!(
        tokio_tungstenite::connect_async(url.as_str())
            .await
            .is_err()
    );
    let mut bad = url.as_str().into_client_request().unwrap();
    bad.headers_mut()
        .insert("authorization", "Bearer nope".parse().unwrap());
    assert!(tokio_tungstenite::connect_async(bad).await.is_err());
    let mut request = url.as_str().into_client_request().unwrap();
    request.headers_mut().insert(
        "authorization",
        format!("Bearer {}", t.token(2)).parse().unwrap(),
    );
    let (mut socket, _) = tokio_tungstenite::connect_async(request).await.unwrap();
    let (_, opened) = t
        .call(1, "POST", "/v1/rooms/direct", json!({ "id": id(2) }))
        .await;
    let room = opened["room"].as_str().unwrap();
    t.call(
        1,
        "POST",
        &format!("/v1/rooms/{room}/messages"),
        json!({ "body": "over the wire" }),
    )
    .await;
    let event = loop {
        let frame = tokio::time::timeout(std::time::Duration::from_secs(5), socket.next())
            .await
            .unwrap()
            .unwrap()
            .unwrap();
        if let tokio_tungstenite::tungstenite::Message::Text(text) = frame {
            break serde_json::from_str::<Value>(&text).unwrap();
        }
    };
    assert_eq!(event["type"], "message");
    assert_eq!(event["message"]["body"], "over the wire");
    assert_eq!(event["message"]["sender"], id(1));
}
