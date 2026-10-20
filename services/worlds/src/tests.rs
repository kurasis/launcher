//! The worlds API through its router, with tokens signed as the account
//! service signs them and devices that sign their own DPoP proofs.

use std::sync::Arc;

use axum::Router;
use axum::body::Body;
use axum::http::{Request, StatusCode, header};
use ed25519_dalek::SigningKey;
use http_body_util::BodyExt;
use serde_json::{Value, json};
use tower::ServiceExt;

use crate::api::{App, Clock, Limiter, router};
use crate::auth::testing::{jwk, proof, sign};
use crate::auth::{Replays, Tokens};
use crate::hub::{Event, Hub};
use crate::store::Store;
use crate::tickets::TicketKey;

const ISSUER: &str = "https://oauth.example";
const BASE: &str = "https://worlds.example";
const AUTHORITY: &str = "authority-epoch-one";
const RELAY_TOKEN: &str = "relay-token-0123456789abcdef0123456789";

const NICKS: [&str; 7] = ["", "Alice", "Bob", "Carol", "Dave", "Erin", "Mallory"];

fn account(n: u8) -> String {
    format!("00000000-0000-4000-8000-0000000000{n:02x}")
}

/// Each person's computer.
fn device(n: u8) -> SigningKey {
    SigningKey::from_bytes(&[100 + n; 32])
}

/// Each person's tunnel key for this session.
fn endpoint(n: u8) -> String {
    format!("{n:02x}").repeat(32)
}

const WORLD: &str = "11111111-1111-4111-8111-111111111111";

struct T {
    dir: tempfile::TempDir,
    app: Arc<App>,
    router: Router,
    issuer: SigningKey,
}

fn app_in(dir: &std::path::Path, authority: &str, issuer: &SigningKey) -> Arc<App> {
    let tokens = Tokens::new(ISSUER);
    tokens.set_jwks(&json!({ "keys": [jwk(issuer)] }));
    Arc::new(App {
        store: Store::open(dir, authority).unwrap(),
        tokens,
        tickets: TicketKey::load_or_create(&dir.join("ticket-key")).unwrap(),
        replays: Replays::default(),
        hub: Hub::default(),
        limiter: Limiter::default(),
        clock: Clock::default(),
        base: BASE.to_owned(),
        relay_token: Some(RELAY_TOKEN.to_owned()),
    })
}

fn setup() -> T {
    let dir = tempfile::tempdir().unwrap();
    let issuer = SigningKey::from_bytes(&[5; 32]);
    let app = app_in(dir.path(), AUTHORITY, &issuer);
    T {
        router: router(Arc::clone(&app)),
        app,
        dir,
        issuer,
    }
}

impl T {
    fn token_with(&self, n: u8, nick: &str, aud: &str) -> String {
        let now = self.app.clock.now();
        sign(
            &self.issuer,
            &json!({"alg": "EdDSA", "typ": "JWT"}),
            &json!({
                "iss": ISSUER, "sub": account(n), "aud": ["account", aud],
                "iat": now, "exp": now + 600, "sid": format!("s{n}"),
                "nick": nick, "ev": true,
            }),
        )
    }

    fn token(&self, n: u8) -> String {
        self.token_with(n, NICKS[n as usize], "worlds")
    }

    async fn send(&self, request: Request<Body>) -> (StatusCode, Value) {
        let response = self.router.clone().oneshot(request).await.unwrap();
        let status = response.status();
        let bytes = response.into_body().collect().await.unwrap().to_bytes();
        (
            status,
            serde_json::from_slice(&bytes).unwrap_or(Value::Null),
        )
    }

    /// A call as person `n`, signed by `key` when it is a device call.
    async fn call_as(
        &self,
        method: &str,
        path: &str,
        token: &str,
        key: Option<&SigningKey>,
        body: Option<Value>,
    ) -> (StatusCode, Value) {
        let mut request = Request::builder()
            .method(method)
            .uri(path)
            .header(header::AUTHORIZATION, format!("Bearer {token}"));
        if let Some(key) = key {
            let url = format!("{BASE}{path}");
            request = request.header(
                "dpop",
                proof(key, method, &url, token, self.app.clock.now()),
            );
        }
        let body = match body {
            Some(value) => {
                request = request.header(header::CONTENT_TYPE, "application/json");
                Body::from(value.to_string())
            }
            None => Body::empty(),
        };
        self.send(request.body(body).unwrap()).await
    }

    async fn call(
        &self,
        method: &str,
        path: &str,
        n: u8,
        body: Option<Value>,
    ) -> (StatusCode, Value) {
        self.call_as(method, path, &self.token(n), None, body).await
    }

    /// A call from person `n`'s computer.
    async fn on_device(
        &self,
        method: &str,
        path: &str,
        n: u8,
        body: Option<Value>,
    ) -> (StatusCode, Value) {
        self.call_as(method, path, &self.token(n), Some(&device(n)), body)
            .await
    }

    async fn register(&self, n: u8) -> String {
        let (status, body) = self
            .on_device("POST", "/v1/devices", n, Some(json!({"name": "Desk"})))
            .await;
        assert_eq!(status, StatusCode::OK, "{body}");
        body["id"].as_str().unwrap().to_owned()
    }

    /// Alice's room, with the people given as members.
    async fn room_with(&self, members: &[u8]) -> String {
        let (status, room) = self
            .call(
                "POST",
                "/v1/rooms",
                1,
                Some(json!({"name": "Our world", "world": WORLD, "minecraft": "1.21.1"})),
            )
            .await;
        assert_eq!(status, StatusCode::OK, "{room}");
        let id = room["id"].as_str().unwrap().to_owned();
        for n in members {
            let code = self.invite(&id, 1, json!({})).await;
            let (status, body) = self
                .call(
                    "POST",
                    "/v1/invites/redeem",
                    *n,
                    Some(json!({"code": code})),
                )
                .await;
            assert_eq!(status, StatusCode::OK, "{body}");
        }
        id
    }

    async fn invite(&self, room: &str, by: u8, body: Value) -> String {
        let (status, made) = self
            .call("POST", &format!("/v1/rooms/{room}/invites"), by, Some(body))
            .await;
        assert_eq!(status, StatusCode::OK, "{made}");
        made["code"].as_str().unwrap().to_owned()
    }

    /// Alice hosts `room` from her computer; returns the session.
    async fn host(&self, room: &str, max_players: i64) -> Value {
        let (status, session) = self
            .on_device(
                "POST",
                &format!("/v1/rooms/{room}/sessions"),
                1,
                Some(json!({"endpoint": endpoint(1), "maxPlayers": max_players})),
            )
            .await;
        assert_eq!(status, StatusCode::OK, "{session}");
        session
    }

    async fn ticket(&self, room: &str, n: u8) -> (StatusCode, Value) {
        self.on_device(
            "POST",
            &format!("/v1/rooms/{room}/tickets"),
            n,
            Some(json!({"endpoint": endpoint(n)})),
        )
        .await
    }

    async fn admit(&self, session: &str, ticket: &str, peer: &str) -> (StatusCode, Value) {
        self.on_device(
            "POST",
            &format!("/v1/sessions/{session}/admissions"),
            1,
            Some(json!({"ticket": ticket, "peer": peer})),
        )
        .await
    }

    async fn renew(&self, session: &str) -> (StatusCode, Value) {
        self.on_device("POST", &format!("/v1/sessions/{session}/renew"), 1, None)
            .await
    }
}

fn error(body: &Value) -> &str {
    body["error"].as_str().unwrap_or_default()
}

#[tokio::test]
async fn devices_need_a_fresh_proof_of_their_key() {
    let t = setup();
    let path = "/v1/devices";
    let token = t.token(1);
    let (status, body) = t
        .call_as("POST", path, &token, None, Some(json!({"name": "Desk"})))
        .await;
    assert_eq!(
        (status, error(&body)),
        (StatusCode::UNAUTHORIZED, "invalid_proof")
    );
    // A proof for another URL, another token or a reused one fails.
    let wrong_url = proof(
        &device(1),
        "POST",
        &format!("{BASE}/v1/rooms"),
        &token,
        t.app.clock.now(),
    );
    let request = |dpop: &str| {
        Request::post(path)
            .header(header::AUTHORIZATION, format!("Bearer {token}"))
            .header(header::CONTENT_TYPE, "application/json")
            .header("dpop", dpop)
            .body(Body::from(r#"{"name":"Desk"}"#))
            .unwrap()
    };
    assert_eq!(
        t.send(request(&wrong_url)).await.0,
        StatusCode::UNAUTHORIZED
    );
    let good = proof(
        &device(1),
        "POST",
        &format!("{BASE}{path}"),
        &token,
        t.app.clock.now(),
    );
    assert_eq!(t.send(request(&good)).await.0, StatusCode::OK);
    assert_eq!(
        t.send(request(&good)).await.0,
        StatusCode::UNAUTHORIZED,
        "replay"
    );
    let stale = proof(
        &device(1),
        "POST",
        &format!("{BASE}{path}"),
        &token,
        t.app.clock.now() - 120,
    );
    assert_eq!(t.send(request(&stale)).await.0, StatusCode::UNAUTHORIZED);

    // The same key again renames; another account cannot take it.
    let first = t.register(1).await;
    assert_eq!(t.register(1).await, first);
    let (status, body) = t
        .call_as(
            "POST",
            path,
            &t.token(2),
            Some(&device(1)),
            Some(json!({"name": "Mine"})),
        )
        .await;
    assert_eq!(
        (status, error(&body)),
        (StatusCode::CONFLICT, "device_key_taken")
    );

    let (_, list) = t.call("GET", path, 1, None).await;
    assert_eq!(list.as_array().unwrap().len(), 1);
    let (status, _) = t.call("DELETE", &format!("{path}/{first}"), 2, None).await;
    assert_eq!(status, StatusCode::NOT_FOUND, "only its owner revokes it");
    let (status, _) = t.call("DELETE", &format!("{path}/{first}"), 1, None).await;
    assert_eq!(status, StatusCode::OK);
    // A revoked key stays revoked.
    let (status, body) = t
        .on_device("POST", path, 1, Some(json!({"name": "Desk"})))
        .await;
    assert_eq!(
        (status, error(&body)),
        (StatusCode::CONFLICT, "device_key_taken")
    );
}

#[tokio::test]
async fn tokens_need_the_audience_and_a_name_the_game_takes() {
    let t = setup();
    let (status, _) = t
        .call_as(
            "GET",
            "/v1/rooms",
            &t.token_with(1, "Alice", "friends"),
            None,
            None,
        )
        .await;
    assert_eq!(status, StatusCode::UNAUTHORIZED);
    let (status, body) = t
        .call_as(
            "GET",
            "/v1/rooms",
            &t.token_with(1, "Ст", "worlds"),
            None,
            None,
        )
        .await;
    assert_eq!(
        (status, error(&body)),
        (StatusCode::FORBIDDEN, "nick_unfit")
    );
    let request = Request::get("/v1/rooms").body(Body::empty()).unwrap();
    assert_eq!(t.send(request).await.0, StatusCode::UNAUTHORIZED);
}

#[tokio::test]
async fn the_permission_table() {
    let t = setup();
    // Alice owns; Bob is made a manager; Carol and Dave are members; Erin is
    // outside.
    let room = t.room_with(&[2, 3, 4]).await;
    let path = format!("/v1/rooms/{room}");
    let (status, _) = t
        .call(
            "POST",
            &format!("{path}/members/{}/role", account(2)),
            3,
            Some(json!({"role": "manager"})),
        )
        .await;
    assert_eq!(status, StatusCode::FORBIDDEN, "a member gives no roles");
    let (status, _) = t
        .call(
            "POST",
            &format!("{path}/members/{}/role", account(2)),
            1,
            Some(json!({"role": "manager"})),
        )
        .await;
    assert_eq!(status, StatusCode::OK);
    let (status, _) = t
        .call(
            "POST",
            &format!("{path}/members/{}/role", account(3)),
            2,
            Some(json!({"role": "manager"})),
        )
        .await;
    assert_eq!(status, StatusCode::FORBIDDEN, "a manager gives no roles");
    let (status, _) = t
        .call(
            "POST",
            &format!("{path}/members/{}/role", account(3)),
            1,
            Some(json!({"role": "owner"})),
        )
        .await;
    assert_eq!(
        status,
        StatusCode::BAD_REQUEST,
        "ownership is not handed out here"
    );

    // Outsiders see nothing, and cannot tell a room from no room.
    for (method, sub) in [
        ("GET", ""),
        ("GET", "/members"),
        ("GET", "/invites"),
        ("DELETE", ""),
    ] {
        let (status, body) = t.call(method, &format!("{path}{sub}"), 5, None).await;
        assert_eq!(
            (status, error(&body)),
            (StatusCode::NOT_FOUND, "not_found"),
            "{method} {sub}"
        );
    }

    // Renaming takes a manager and the current revision.
    let (_, current) = t.call("GET", &path, 3, None).await;
    let revision = current["revision"].as_i64().unwrap();
    assert!(
        current.get("world").is_none(),
        "only the owner sees the world ID"
    );
    let (status, _) = t
        .call(
            "PATCH",
            &path,
            3,
            Some(json!({"revision": revision, "name": "Mine"})),
        )
        .await;
    assert_eq!(status, StatusCode::FORBIDDEN);
    let (status, renamed) = t
        .call(
            "PATCH",
            &path,
            2,
            Some(json!({"revision": revision, "name": "Ours"})),
        )
        .await;
    assert_eq!(
        (status, renamed["name"].as_str()),
        (StatusCode::OK, Some("Ours"))
    );
    let (status, body) = t
        .call(
            "PATCH",
            &path,
            1,
            Some(json!({"revision": revision, "description": "x"})),
        )
        .await;
    assert_eq!(
        (status, error(&body)),
        (StatusCode::CONFLICT, "revision_conflict")
    );

    // Invites: managers and the owner.
    let (status, _) = t
        .call("POST", &format!("{path}/invites"), 3, Some(json!({})))
        .await;
    assert_eq!(status, StatusCode::FORBIDDEN);
    t.invite(&room, 2, json!({})).await;

    // Removing: a member removes nobody, a manager removes members only, no
    // one removes the owner.
    let remove = |who: u8| format!("{path}/members/{}/remove", account(who));
    assert_eq!(
        t.call("POST", &remove(4), 3, None).await.0,
        StatusCode::FORBIDDEN
    );
    assert_eq!(
        t.call("POST", &remove(1), 2, None).await.0,
        StatusCode::FORBIDDEN
    );
    assert_eq!(t.call("POST", &remove(4), 2, None).await.0, StatusCode::OK);
    assert_eq!(t.call("POST", &remove(2), 1, None).await.0, StatusCode::OK);
    let (status, body) = t.call("GET", &path, 2, None).await;
    assert_eq!(
        (status, error(&body)),
        (StatusCode::NOT_FOUND, "not_found"),
        "removed"
    );

    // Members see the active members; the owner sees everyone.
    let (_, seen) = t.call("GET", &format!("{path}/members"), 3, None).await;
    assert_eq!(seen.as_array().unwrap().len(), 2);
    let (_, all) = t.call("GET", &format!("{path}/members"), 1, None).await;
    let states: Vec<&str> = all
        .as_array()
        .unwrap()
        .iter()
        .map(|m| m["state"].as_str().unwrap())
        .collect();
    assert_eq!(states, ["active", "removed", "active", "removed"]);

    // The owner cannot leave, a member can; only the owner deletes.
    let (status, body) = t.call("POST", &format!("{path}/leave"), 1, None).await;
    assert_eq!(
        (status, error(&body)),
        (StatusCode::CONFLICT, "owner_cannot_leave")
    );
    assert_eq!(
        t.call("DELETE", &path, 3, None).await.0,
        StatusCode::FORBIDDEN
    );
    assert_eq!(
        t.call("POST", &format!("{path}/leave"), 3, None).await.0,
        StatusCode::OK
    );
    assert_eq!(t.call("DELETE", &path, 1, None).await.0, StatusCode::OK);
    assert_eq!(t.call("GET", &path, 1, None).await.0, StatusCode::NOT_FOUND);
    let (_, rooms) = t.call("GET", "/v1/rooms", 1, None).await;
    assert_eq!(rooms, json!([]));
}

#[tokio::test]
async fn invites_are_bounded_single_use_and_end() {
    let t = setup();
    let room = t.room_with(&[]).await;
    let path = format!("/v1/rooms/{room}/invites");
    for bad in [
        json!({"hours": 0}),
        json!({"hours": 169}),
        json!({"uses": 0}),
        json!({"uses": 11}),
        json!({"hours": "24"}),
        json!({"forever": true}),
    ] {
        let (status, _) = t.call("POST", &path, 1, Some(bad.clone())).await;
        assert_eq!(status, StatusCode::BAD_REQUEST, "{bad}");
    }
    let (_, made) = t.call("POST", &path, 1, Some(json!({}))).await;
    let created = made["invite"]["created"].as_i64().unwrap();
    assert_eq!(
        made["invite"]["expires"].as_i64().unwrap() - created,
        24 * 3600
    );
    assert_eq!(made["invite"]["maxUses"], 1);
    let code = made["code"].as_str().unwrap().to_owned();
    assert_eq!(code.len(), 22);

    let (status, preview) = t
        .call(
            "POST",
            "/v1/invites/preview",
            2,
            Some(json!({"code": code})),
        )
        .await;
    assert_eq!(status, StatusCode::OK);
    assert_eq!(preview["ownerNick"], "Alice");
    assert_eq!(preview["members"], 1);

    let redeem = |n: u8, code: &str| {
        let code = code.to_owned();
        let t = &t;
        async move {
            t.call("POST", "/v1/invites/redeem", n, Some(json!({"code": code})))
                .await
        }
    };
    let (status, joined) = redeem(2, &code).await;
    assert_eq!(
        (status, joined["role"].as_str()),
        (StatusCode::OK, Some("member"))
    );
    // Used up: the next person gets the same answer as for a wrong code.
    let (status, body) = redeem(3, &code).await;
    assert_eq!(
        (status, error(&body)),
        (StatusCode::NOT_FOUND, "invite_invalid")
    );
    let (status, body) = redeem(3, "AAAAAAAAAAAAAAAAAAAAAA").await;
    assert_eq!(
        (status, error(&body)),
        (StatusCode::NOT_FOUND, "invite_invalid")
    );
    for junk in ["", "' OR 1=1 --", "../../etc/passwd", &"A".repeat(5000)] {
        let (status, body) = redeem(3, junk).await;
        assert_eq!(
            (status, error(&body)),
            (StatusCode::NOT_FOUND, "invite_invalid")
        );
    }

    // Someone already in keeps their place without using an invite up.
    let code = t.invite(&room, 1, json!({"uses": 1})).await;
    assert_eq!(redeem(2, &code).await.0, StatusCode::OK);
    assert_eq!(redeem(3, &code).await.0, StatusCode::OK);

    // Expired and revoked invites stop working.
    let code = t.invite(&room, 1, json!({"hours": 1})).await;
    t.app.clock.advance(3600);
    assert_eq!(redeem(4, &code).await.0, StatusCode::NOT_FOUND);
    let (_, made) = t.call("POST", &path, 1, Some(json!({"uses": 5}))).await;
    let (_, open) = t.call("GET", &path, 1, None).await;
    assert_eq!(open.as_array().unwrap().len(), 1);
    let id = made["invite"]["id"].as_str().unwrap();
    assert_eq!(
        t.call("DELETE", &format!("{path}/{id}"), 1, None).await.0,
        StatusCode::OK
    );
    assert_eq!(
        redeem(4, made["code"].as_str().unwrap()).await.0,
        StatusCode::NOT_FOUND
    );

    // A banned account does not come back, until the ban is lifted.
    let ban = format!("/v1/rooms/{room}/members/{}/ban", account(2));
    assert_eq!(t.call("POST", &ban, 1, None).await.0, StatusCode::OK);
    let code = t.invite(&room, 1, json!({"uses": 2})).await;
    let (status, body) = redeem(2, &code).await;
    assert_eq!((status, error(&body)), (StatusCode::FORBIDDEN, "banned"));
    let unban = format!("/v1/rooms/{room}/members/{}/unban", account(2));
    assert_eq!(t.call("POST", &unban, 1, None).await.0, StatusCode::OK);
    assert_eq!(redeem(2, &code).await.0, StatusCode::OK);
}

#[tokio::test]
async fn the_last_use_of_an_invite_is_taken_once() {
    let t = setup();
    let room = t.room_with(&[]).await;
    for uses in [1, 2] {
        let code = t.invite(&room, 1, json!({"uses": uses})).await;
        let tries = [2u8, 3, 4, 5, 6].map(|n| {
            let code = code.clone();
            let t = &t;
            async move {
                t.call("POST", "/v1/invites/redeem", n, Some(json!({"code": code})))
                    .await
                    .0
            }
        });
        let results = futures_util::future::join_all(tries).await;
        let joined = results.iter().filter(|s| **s == StatusCode::OK).count();
        // Some of them may already be members from the round before.
        assert!(joined >= usize::try_from(uses).unwrap(), "{results:?}");
        let (_, members) = t
            .call("GET", &format!("/v1/rooms/{room}/members"), 1, None)
            .await;
        assert_eq!(
            members.as_array().unwrap().len(),
            1 + [1, 3][usize::try_from(uses - 1).unwrap()]
        );
    }
}

#[tokio::test]
async fn one_room_has_one_live_host() {
    let t = setup();
    let room = t.room_with(&[2]).await;
    t.register(1).await;
    t.register(2).await;
    // A member cannot host the owner's world.
    let (status, _) = t
        .on_device(
            "POST",
            &format!("/v1/rooms/{room}/sessions"),
            2,
            Some(json!({"endpoint": endpoint(2), "maxPlayers": 8})),
        )
        .await;
    assert_eq!(status, StatusCode::FORBIDDEN);
    // An unregistered computer cannot host either.
    let laptop = SigningKey::from_bytes(&[42; 32]);
    let (status, body) = t
        .call_as(
            "POST",
            &format!("/v1/rooms/{room}/sessions"),
            &t.token(1),
            Some(&laptop),
            Some(json!({"endpoint": endpoint(1), "maxPlayers": 8})),
        )
        .await;
    assert_eq!(
        (status, error(&body)),
        (StatusCode::FORBIDDEN, "device_unknown")
    );
    for bad in [
        json!({"endpoint": "x", "maxPlayers": 8}),
        json!({"endpoint": endpoint(1), "maxPlayers": 0}),
        json!({"endpoint": endpoint(1), "maxPlayers": 21}),
        json!({"endpoint": endpoint(1)}),
    ] {
        let (status, _) = t
            .on_device(
                "POST",
                &format!("/v1/rooms/{room}/sessions"),
                1,
                Some(bad.clone()),
            )
            .await;
        assert_eq!(status, StatusCode::BAD_REQUEST, "{bad}");
    }

    // Two starts at once: both are answered, the later one supersedes, and
    // only one session is live.
    let (a, b) = tokio::join!(t.host(&room, 8), t.host(&room, 8));
    let (first, second) = if a["epoch"].as_i64() < b["epoch"].as_i64() {
        (a, b)
    } else {
        (b, a)
    };
    assert_eq!(
        second["epoch"].as_i64().unwrap(),
        first["epoch"].as_i64().unwrap() + 1
    );
    let (status, body) = t.renew(first["id"].as_str().unwrap()).await;
    assert_eq!(
        (status, error(&body)),
        (StatusCode::CONFLICT, "session_ended")
    );
    let (status, renewal) = t.renew(second["id"].as_str().unwrap()).await;
    assert_eq!(status, StatusCode::OK);
    assert_eq!(renewal["members"], json!(["Alice", "Bob"]));
    assert_eq!(t.app.store.counts(t.app.clock.now()).unwrap()["hosting"], 1);

    // Another computer of the owner cannot renew or stop this session.
    let (status, _) = t
        .call_as(
            "POST",
            &format!("/v1/sessions/{}/renew", second["id"].as_str().unwrap()),
            &t.token(2),
            Some(&device(2)),
            None,
        )
        .await;
    assert_eq!(status, StatusCode::NOT_FOUND);
    let stop = format!("/v1/sessions/{}/stop", second["id"].as_str().unwrap());
    assert_eq!(t.on_device("POST", &stop, 1, None).await.0, StatusCode::OK);
    let (_, room_now) = t.call("GET", &format!("/v1/rooms/{room}"), 2, None).await;
    assert_eq!(room_now["hosting"], false);
}

#[tokio::test]
async fn tickets_admit_one_member_once() {
    let t = setup();
    let room = t.room_with(&[2, 3]).await;
    for n in 1..=3 {
        t.register(n).await;
    }
    let (status, body) = t.ticket(&room, 2).await;
    assert_eq!(
        (status, error(&body)),
        (StatusCode::CONFLICT, "not_hosting")
    );
    let session = t.host(&room, 2).await;
    let session_id = session["id"].as_str().unwrap();

    // Outsiders and unregistered computers get no ticket.
    t.register(5).await;
    assert_eq!(t.ticket(&room, 5).await.0, StatusCode::NOT_FOUND);
    let (status, _) = t
        .call_as(
            "POST",
            &format!("/v1/rooms/{room}/tickets"),
            &t.token(2),
            Some(&SigningKey::from_bytes(&[43; 32])),
            Some(json!({"endpoint": endpoint(2)})),
        )
        .await;
    assert_eq!(status, StatusCode::FORBIDDEN);

    let (status, grant) = t.ticket(&room, 2).await;
    assert_eq!(status, StatusCode::OK, "{grant}");
    assert_eq!(grant["session"], session["id"]);
    assert_eq!(
        grant["hostEpoch"],
        session["epoch"].as_i64().unwrap().to_string()
    );
    assert_eq!(grant["hostEndpoint"], endpoint(1));
    let ticket = grant["ticket"].as_str().unwrap();

    // A guest's computer cannot consume tickets for the host.
    let (status, _) = t
        .call_as(
            "POST",
            &format!("/v1/sessions/{session_id}/admissions"),
            &t.token(2),
            Some(&device(2)),
            Some(json!({"ticket": ticket, "peer": endpoint(2)})),
        )
        .await;
    assert_eq!(status, StatusCode::NOT_FOUND);

    let (status, admitted) = t.admit(session_id, ticket, &endpoint(2)).await;
    assert_eq!(status, StatusCode::OK, "{admitted}");
    assert_eq!(admitted["nick"], "Bob");
    assert_eq!(admitted["account"], account(2));
    // Replayed: used up.
    let (status, body) = t.admit(session_id, ticket, &endpoint(2)).await;
    assert_eq!(
        (status, error(&body)),
        (StatusCode::FORBIDDEN, "ticket_invalid")
    );

    // From another tunnel key: refused, and the ticket is gone.
    let (_, grant) = t.ticket(&room, 3).await;
    let stolen = grant["ticket"].as_str().unwrap();
    let (status, body) = t.admit(session_id, stolen, &endpoint(6)).await;
    assert_eq!(
        (status, error(&body)),
        (StatusCode::FORBIDDEN, "ticket_invalid")
    );
    let (status, _) = t.admit(session_id, stolen, &endpoint(3)).await;
    assert_eq!(status, StatusCode::FORBIDDEN);

    // One slot per account until the host releases it.
    let (_, again) = t.ticket(&room, 2).await;
    let (status, body) = t
        .admit(session_id, again["ticket"].as_str().unwrap(), &endpoint(2))
        .await;
    assert_eq!(
        (status, error(&body)),
        (StatusCode::CONFLICT, "already_connected")
    );

    // The room is full at two players.
    let (_, owner) = t.ticket(&room, 1).await;
    assert_eq!(
        t.admit(session_id, owner["ticket"].as_str().unwrap(), &endpoint(1))
            .await
            .0,
        StatusCode::OK
    );
    let (_, carol) = t.ticket(&room, 3).await;
    let (status, body) = t
        .admit(session_id, carol["ticket"].as_str().unwrap(), &endpoint(3))
        .await;
    assert_eq!((status, error(&body)), (StatusCode::CONFLICT, "room_full"));
    let (_, shown) = t.call("GET", &format!("/v1/rooms/{room}"), 3, None).await;
    assert_eq!(
        (shown["online"].as_i64(), shown["maxPlayers"].as_i64()),
        (Some(2), Some(2))
    );
    // The host counts players on its server, its own game included; the
    // room shows the larger of that and the open admissions.
    let renew = format!("/v1/sessions/{session_id}/renew");
    assert_eq!(
        t.on_device("POST", &renew, 1, Some(json!({"players": 3})))
            .await
            .0,
        StatusCode::OK
    );
    let (_, shown) = t.call("GET", &format!("/v1/rooms/{room}"), 3, None).await;
    assert_eq!(shown["online"].as_i64(), Some(3));
    // A renewal without a count keeps the last one.
    assert_eq!(t.renew(session_id).await.0, StatusCode::OK);
    let (_, shown) = t.call("GET", &format!("/v1/rooms/{room}"), 3, None).await;
    assert_eq!(shown["online"].as_i64(), Some(3));
    for junk in [
        json!({"players": 1001}),
        json!({"players": -1}),
        json!({"players": "3"}),
        json!({"players": 1, "extra": true}),
    ] {
        let (status, body) = t.on_device("POST", &renew, 1, Some(junk)).await;
        assert_eq!(
            (status, error(&body)),
            (StatusCode::BAD_REQUEST, "bad_request")
        );
    }
    assert_eq!(
        t.on_device("POST", &renew, 1, Some(json!({"players": 0})))
            .await
            .0,
        StatusCode::OK
    );

    let release = format!(
        "/v1/sessions/{session_id}/admissions/{}/release",
        admitted["id"].as_str().unwrap()
    );
    assert_eq!(
        t.on_device("POST", &release, 1, None).await.0,
        StatusCode::OK
    );
    assert_eq!(
        t.on_device("POST", &release, 1, None).await.0,
        StatusCode::NOT_FOUND
    );
    let (_, carol) = t.ticket(&room, 3).await;
    assert_eq!(
        t.admit(session_id, carol["ticket"].as_str().unwrap(), &endpoint(3))
            .await
            .0,
        StatusCode::OK
    );

    // A ticket lives 60 seconds; the lease is kept alive meanwhile.
    let (_, late) = t.ticket(&room, 2).await;
    t.app.clock.advance(25);
    assert_eq!(t.renew(session_id).await.0, StatusCode::OK);
    t.app.clock.advance(25);
    assert_eq!(t.renew(session_id).await.0, StatusCode::OK);
    t.app.clock.advance(11);
    let (status, body) = t
        .admit(session_id, late["ticket"].as_str().unwrap(), &endpoint(2))
        .await;
    assert_eq!(
        (status, error(&body)),
        (StatusCode::FORBIDDEN, "ticket_invalid")
    );

    // A forged ticket (signed by another key) or junk is refused.
    for junk in ["", "a.b.c", "' OR 1=1 --", &"x".repeat(5000)] {
        let (status, _) = t.admit(session_id, junk, &endpoint(2)).await;
        assert_eq!(status, StatusCode::FORBIDDEN);
    }
}

#[tokio::test]
async fn a_late_renewal_finds_the_session_ended() {
    let t = setup();
    let room = t.room_with(&[2]).await;
    t.register(1).await;
    t.register(2).await;
    let session = t.host(&room, 8).await;
    let session_id = session["id"].as_str().unwrap();
    let (_, grant) = t.ticket(&room, 2).await;
    t.app.clock.advance(31);
    let (status, body) = t.renew(session_id).await;
    assert_eq!(
        (status, error(&body)),
        (StatusCode::CONFLICT, "session_ended")
    );
    let (status, body) = t
        .admit(session_id, grant["ticket"].as_str().unwrap(), &endpoint(2))
        .await;
    assert_eq!(
        (status, error(&body)),
        (StatusCode::CONFLICT, "session_ended")
    );
    let (status, body) = t.ticket(&room, 2).await;
    assert_eq!(
        (status, error(&body)),
        (StatusCode::CONFLICT, "not_hosting")
    );
    let (_, shown) = t.call("GET", &format!("/v1/rooms/{room}"), 2, None).await;
    assert_eq!(shown["hosting"], false);
    // Hosting again starts a new epoch.
    let again = t.host(&room, 8).await;
    assert_eq!(
        again["epoch"].as_i64(),
        Some(session["epoch"].as_i64().unwrap() + 1)
    );
}

#[tokio::test]
async fn removed_members_are_closed_at_the_next_renewal() {
    let t = setup();
    let room = t.room_with(&[2, 3]).await;
    for n in 1..=3 {
        t.register(n).await;
    }
    let session = t.host(&room, 8).await;
    let session_id = session["id"].as_str().unwrap();
    let (_, grant) = t.ticket(&room, 2).await;
    let (_, admitted) = t
        .admit(session_id, grant["ticket"].as_str().unwrap(), &endpoint(2))
        .await;
    // Carol holds a ticket she has not used yet.
    let (_, pending) = t.ticket(&room, 3).await;

    let (events_id, mut events) = t.app.hub.join(&account(1)).unwrap();
    let remove = |n: u8| format!("/v1/rooms/{room}/members/{}/remove", account(n));
    assert_eq!(t.call("POST", &remove(2), 1, None).await.0, StatusCode::OK);
    assert_eq!(t.call("POST", &remove(3), 1, None).await.0, StatusCode::OK);
    // The host hears at once and renews.
    assert_eq!(events.try_recv().unwrap(), Event::Room { id: room.clone() });
    t.app.hub.leave(&account(1), events_id);
    let (_, renewal) = t.renew(session_id).await;
    assert_eq!(renewal["revoked"], json!([admitted["id"]]));
    assert_eq!(renewal["members"], json!(["Alice"]));
    let (status, body) = t
        .admit(
            session_id,
            pending["ticket"].as_str().unwrap(),
            &endpoint(3),
        )
        .await;
    assert_eq!(
        (status, error(&body)),
        (StatusCode::FORBIDDEN, "not_member")
    );
    assert_eq!(t.ticket(&room, 2).await.0, StatusCode::NOT_FOUND);
}

#[tokio::test]
async fn a_restored_database_revives_no_lease() {
    let t = setup();
    let room = t.room_with(&[2]).await;
    t.register(1).await;
    t.register(2).await;
    let session = t.host(&room, 8).await;
    let (_, grant) = t.ticket(&room, 2).await;
    // The same rows under a new authority epoch.
    let restored = app_in(t.dir.path(), "authority-epoch-two", &t.issuer);
    let t2 = T {
        router: router(Arc::clone(&restored)),
        app: restored,
        dir: tempfile::tempdir().unwrap(),
        issuer: t.issuer.clone(),
    };
    let session_id = session["id"].as_str().unwrap();
    let (status, body) = t2.renew(session_id).await;
    assert_eq!(
        (status, error(&body)),
        (StatusCode::CONFLICT, "session_ended")
    );
    let (status, _) = t2
        .admit(session_id, grant["ticket"].as_str().unwrap(), &endpoint(2))
        .await;
    assert_eq!(status, StatusCode::CONFLICT);
}

#[tokio::test]
async fn junk_is_refused() {
    let t = setup();
    let room = t.room_with(&[]).await;
    let token = t.token(1);
    for (path, body) in [
        ("/v1/rooms", "{"),
        ("/v1/rooms", "[]"),
        (
            "/v1/rooms",
            r#"{"name":"x","world":"11111111-1111-4111-8111-111111111112","minecraft":"1.21.1","admin":true}"#,
        ),
        (
            "/v1/rooms",
            r#"{"name":"","world":"11111111-1111-4111-8111-111111111112","minecraft":"1.21.1"}"#,
        ),
        (
            "/v1/rooms",
            r#"{"name":"x","world":"../../etc","minecraft":"1.21.1"}"#,
        ),
        (
            "/v1/rooms",
            r#"{"name":"x","world":"11111111-1111-4111-8111-111111111112","minecraft":"1.8.9"}"#,
        ),
    ] {
        let request = Request::post(path)
            .header(header::AUTHORIZATION, format!("Bearer {token}"))
            .header(header::CONTENT_TYPE, "application/json")
            .body(Body::from(body))
            .unwrap();
        assert_eq!(t.send(request).await.0, StatusCode::BAD_REQUEST, "{body}");
    }
    let long = json!({"name": "x".repeat(65), "world": WORLD, "minecraft": "1.21.1"});
    assert_eq!(
        t.call("POST", "/v1/rooms", 1, Some(long)).await.0,
        StatusCode::BAD_REQUEST
    );
    let (status, body) = t
        .call(
            "POST",
            "/v1/rooms",
            1,
            Some(json!({"name": "Again", "world": WORLD, "minecraft": "1.21.1"})),
        )
        .await;
    assert_eq!(
        (status, error(&body)),
        (StatusCode::CONFLICT, "world_taken")
    );
    // Names lose control and direction characters; markup stays text.
    let (_, renamed) = t
        .call(
            "PATCH",
            &format!("/v1/rooms/{room}"),
            1,
            Some(json!({"revision": 1, "name": "<b>\u{202E}A\u{0}B</b>", "description": "line\u{1b}[31m"})),
        )
        .await;
    assert_eq!(renamed["name"], "<b>AB</b>");
    assert_eq!(renamed["description"], "line[31m");
    for path in [
        "/v1/rooms/..%2F..%2Fetc",
        "/v1/rooms/1%27%20OR%201=1",
        "/v1/rooms/ROOM",
    ] {
        assert_eq!(
            t.call("GET", path, 1, None).await.0,
            StatusCode::NOT_FOUND,
            "{path}"
        );
    }
    let huge = Request::post("/v1/rooms")
        .header(header::AUTHORIZATION, format!("Bearer {token}"))
        .header(header::CONTENT_TYPE, "application/json")
        .body(Body::from(vec![b' '; 20_000]))
        .unwrap();
    assert_eq!(t.send(huge).await.0, StatusCode::PAYLOAD_TOO_LARGE);
    let (status, _) = t
        .call(
            "POST",
            &format!("/v1/rooms/{room}/members/{}/promote", account(2)),
            1,
            None,
        )
        .await;
    assert_eq!(status, StatusCode::NOT_FOUND);
}

#[tokio::test]
async fn nicks_follow_the_latest_token() {
    let t = setup();
    let room = t.room_with(&[2]).await;
    let renamed = t.token_with(2, "Robert", "worlds");
    let (_, members) = t
        .call_as(
            "GET",
            &format!("/v1/rooms/{room}/members"),
            &renamed,
            None,
            None,
        )
        .await;
    let nicks: Vec<&str> = members
        .as_array()
        .unwrap()
        .iter()
        .map(|m| m["nick"].as_str().unwrap())
        .collect();
    assert_eq!(nicks, ["Alice", "Robert"]);
}

#[tokio::test]
async fn health_and_ticket_keys_are_public() {
    let t = setup();
    let (status, health) = t
        .send(Request::get("/v1/health").body(Body::empty()).unwrap())
        .await;
    assert_eq!(
        (status, health["keys"].as_bool()),
        (StatusCode::OK, Some(true))
    );
    let (status, keys) = t
        .send(
            Request::get("/.well-known/jwks.json")
                .body(Body::empty())
                .unwrap(),
        )
        .await;
    assert_eq!(status, StatusCode::OK);
    assert_eq!(keys["keys"][0]["crv"], "Ed25519");
    assert!(keys["keys"][0].get("d").is_none(), "no private part");
    let counts = t.app.store.counts(t.app.clock.now()).unwrap();
    assert!(counts.as_object().unwrap().values().all(Value::is_number));
}

#[tokio::test]
async fn the_relay_carries_hosts_and_enrolled_members_only() {
    let t = setup();
    let room = t.room_with(&[2, 3]).await;
    for n in 1..=5 {
        t.register(n).await;
    }
    let allow = || async {
        let request = Request::get("/v1/relay/allow")
            .header(header::AUTHORIZATION, format!("Bearer {RELAY_TOKEN}"))
            .body(Body::empty())
            .unwrap();
        let (status, body) = t.send(request).await;
        assert_eq!(status, StatusCode::OK);
        let mut keys: Vec<String> = body["allow"]
            .as_array()
            .unwrap()
            .iter()
            .map(|g| g["endpoint"].as_str().unwrap().to_owned())
            .collect();
        keys.sort();
        (keys, body)
    };
    let enroll = |n: u8, key: String| {
        let room = room.clone();
        let t = &t;
        async move {
            t.on_device(
                "POST",
                &format!("/v1/rooms/{room}/relay"),
                n,
                Some(json!({"endpoint": key})),
            )
            .await
        }
    };
    // Nothing before hosting.
    let (status, body) = enroll(2, endpoint(2)).await;
    assert_eq!(
        (status, error(&body)),
        (StatusCode::CONFLICT, "not_hosting")
    );
    assert!(allow().await.0.is_empty());

    let session = t.host(&room, 8).await;
    assert_eq!(allow().await.0, [endpoint(1)]);
    let (status, body) = enroll(2, endpoint(2)).await;
    assert_eq!((status, body["ttl"].as_i64()), (StatusCode::OK, Some(30)));
    assert_eq!(enroll(3, endpoint(3)).await.0, StatusCode::OK);
    // Outsiders and junk are refused.
    assert_eq!(enroll(5, endpoint(5)).await.0, StatusCode::NOT_FOUND);
    assert_eq!(enroll(3, "zz".repeat(32)).await.0, StatusCode::BAD_REQUEST);
    let (keys, body) = allow().await;
    assert_eq!(keys, [endpoint(1), endpoint(2), endpoint(3)]);
    let groups: Vec<&str> = body["allow"]
        .as_array()
        .unwrap()
        .iter()
        .map(|g| g["group"].as_str().unwrap())
        .collect();
    assert!(
        groups.iter().all(|g| g.len() == 24 && !g.contains('-')),
        "no account IDs"
    );
    assert!(!body.to_string().contains(&account(2)));

    // A new key replaces the member's old one.
    assert_eq!(enroll(3, endpoint(6)).await.0, StatusCode::OK);
    assert_eq!(allow().await.0, [endpoint(1), endpoint(2), endpoint(6)]);
    // A removed member is dropped at once.
    let remove = format!("/v1/rooms/{room}/members/{}/remove", account(2));
    assert_eq!(t.call("POST", &remove, 1, None).await.0, StatusCode::OK);
    assert_eq!(allow().await.0, [endpoint(1), endpoint(6)]);
    // A guest that stops renewing drops out after 30 seconds; the host stays
    // while it renews.
    t.app.clock.advance(20);
    assert_eq!(
        t.renew(session["id"].as_str().unwrap()).await.0,
        StatusCode::OK
    );
    t.app.clock.advance(11);
    assert_eq!(allow().await.0, [endpoint(1)]);
    // A host that stops renewing drops out with everyone.
    assert_eq!(enroll(3, endpoint(3)).await.0, StatusCode::OK);
    t.app.clock.advance(31);
    assert!(allow().await.0.is_empty());

    // The list needs the relay's token.
    for given in ["", "Bearer wrong", "relay-token-0123456789abcdef0123456789"] {
        let mut request = Request::get("/v1/relay/allow");
        if !given.is_empty() {
            request = request.header(header::AUTHORIZATION, given);
        }
        let (status, _) = t.send(request.body(Body::empty()).unwrap()).await;
        assert_eq!(status, StatusCode::UNAUTHORIZED, "{given}");
    }
}

#[test]
fn an_older_database_gains_the_player_count() {
    let dir = tempfile::tempdir().unwrap();
    {
        let db = rusqlite::Connection::open(dir.path().join("worlds.db")).unwrap();
        db.execute_batch(
            "CREATE TABLE sessions (
                id TEXT PRIMARY KEY, room TEXT NOT NULL, host_device TEXT NOT NULL,
                epoch INTEGER NOT NULL, authority TEXT NOT NULL, endpoint TEXT NOT NULL,
                max_players INTEGER NOT NULL, state TEXT NOT NULL, lease_until INTEGER NOT NULL,
                started INTEGER NOT NULL, ended INTEGER, UNIQUE (room, epoch));",
        )
        .unwrap();
    }
    crate::store::Store::open(dir.path(), "test").unwrap();
    // A second start finds the column there.
    crate::store::Store::open(dir.path(), "test").unwrap();
}
