//! The relay with its gate, over plain HTTP on loopback, and the allow list
//! read from a fake worlds service.

use std::sync::Arc;
use std::time::{Duration, Instant};

use axum::http::{HeaderMap, StatusCode};
use iroh_base::{RelayUrl, SecretKey};
use iroh_dns::dns::DnsResolver;
use iroh_relay::client::ClientBuilder;
use iroh_relay::server::Server;
use iroh_relay::tls::{CaTlsConfig, default_provider};
use n0_future::StreamExt;

use crate::gate::Gate;
use crate::{Settings, close_outlawed, http_client, rate_limit, refresh, server_config};

const TOKEN: &str = "relay-token-0123456789abcdef0123456789";

async fn local(gate: &Arc<Gate>) -> Server {
    let settings = Settings {
        http: "127.0.0.1:0".parse().unwrap(),
        tls: None,
        metrics: None,
        rate: rate_limit(1 << 20),
    };
    Server::spawn(server_config(settings, Arc::clone(gate)))
        .await
        .unwrap()
}

#[tokio::test]
async fn only_listed_keys_are_carried_and_dropped_ones_are_closed() {
    let gate = Arc::new(Gate::new(8));
    let allowed = SecretKey::from_bytes(&[1; 32]);
    let stranger = SecretKey::from_bytes(&[2; 32]);
    gate.replace(
        vec![(allowed.public(), "g1".into(), Duration::from_secs(30))],
        Instant::now(),
    );
    let server = local(&gate).await;
    let url: RelayUrl = format!("http://{}", server.http_addr().unwrap())
        .parse()
        .unwrap();
    let tls = CaTlsConfig::default()
        .client_config(default_provider())
        .unwrap();
    let connect = |key: SecretKey| {
        let builder =
            ClientBuilder::new(url.clone(), key, DnsResolver::new()).tls_client_config(tls.clone());
        async move { builder.connect().await }
    };
    let mut client = connect(allowed.clone())
        .await
        .expect("a listed key connects");
    assert!(
        connect(stranger).await.is_err(),
        "an unlisted key is refused"
    );
    assert_eq!(gate.connections(), 1);

    // Still listed: nothing is closed.
    let clients = server.relay_service().unwrap().clients().clone();
    assert_eq!(close_outlawed(&clients, &gate), 0);

    // The member was removed: the busy connection is closed.
    gate.replace(Vec::new(), Instant::now());
    assert_eq!(close_outlawed(&clients, &gate), 1);
    let ended = tokio::time::timeout(Duration::from_secs(10), async {
        loop {
            match client.next().await {
                None | Some(Err(_)) => break,
                Some(Ok(_)) => {}
            }
        }
    })
    .await;
    assert!(ended.is_ok(), "the connection ends");
    // Its place is given back, and it cannot come back.
    for _ in 0..50 {
        if gate.connections() == 0 {
            break;
        }
        tokio::time::sleep(Duration::from_millis(100)).await;
    }
    assert_eq!(gate.connections(), 0);
    assert!(connect(allowed).await.is_err());
}

#[tokio::test]
async fn a_record_that_is_not_renewed_closes_the_connection() {
    let gate = Arc::new(Gate::new(8));
    let key = SecretKey::from_bytes(&[3; 32]);
    gate.replace(
        vec![(key.public(), "g1".into(), Duration::from_secs(1))],
        Instant::now(),
    );
    let server = local(&gate).await;
    let url: RelayUrl = format!("http://{}", server.http_addr().unwrap())
        .parse()
        .unwrap();
    let tls = CaTlsConfig::default()
        .client_config(default_provider())
        .unwrap();
    let _client = ClientBuilder::new(url, key, DnsResolver::new())
        .tls_client_config(tls)
        .connect()
        .await
        .unwrap();
    let clients = server.relay_service().unwrap().clients().clone();
    assert_eq!(close_outlawed(&clients, &gate), 0);
    tokio::time::sleep(Duration::from_millis(1100)).await;
    assert_eq!(close_outlawed(&clients, &gate), 1);
}

#[tokio::test]
async fn the_list_is_read_with_the_token() {
    let key = SecretKey::from_bytes(&[4; 32]).public().to_string();
    let body =
        serde_json::json!({"allow": [{"endpoint": key, "group": "g1", "ttl": 30}]}).to_string();
    let app = axum::Router::new().route(
        "/v1/relay/allow",
        axum::routing::get(move |headers: HeaderMap| {
            let body = body.clone();
            async move {
                let expected = format!("Bearer {TOKEN}");
                if headers.get("authorization").and_then(|v| v.to_str().ok()) == Some(&expected) {
                    (StatusCode::OK, body)
                } else {
                    (StatusCode::UNAUTHORIZED, String::new())
                }
            }
        }),
    );
    let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
    let url = format!("http://{}/v1/relay/allow", listener.local_addr().unwrap());
    tokio::spawn(async move { axum::serve(listener, app).await });

    let gate = Gate::new(8);
    let http = http_client().unwrap();
    assert_eq!(refresh(&http, &url, TOKEN, &gate).await, Ok(1));
    let wrong = refresh(&http, &url, "another-token-0123456789abcdef0123", &gate).await;
    assert_eq!(wrong, Err("the allow list answered 401".to_owned()));
    // A failed read keeps the records it had.
    let listed = SecretKey::from_bytes(&[4; 32]).public();
    assert!(gate.admit(listed, connection_id(), Instant::now()));
    let missing = refresh(&http, "http://127.0.0.1:9/none", TOKEN, &gate).await;
    assert!(missing.is_err());
}

fn connection_id() -> iroh_relay::server::ConnectionId {
    let parts = http::Request::new(()).into_parts().0;
    iroh_relay::server::ClientRequest::new(
        SecretKey::from_bytes(&[0; 32]).public(),
        Default::default(),
        parts,
    )
    .connection_id()
}
