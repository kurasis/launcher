//! A host and a guest on loopback through the real tunnel, with the fake
//! worlds service and a stand-in world server.

use std::sync::{Arc, Mutex};
use std::time::Duration;

use tokio::io::{AsyncReadExt, AsyncWriteExt};
use tokio::net::{TcpListener, TcpStream};
use tokio::sync::mpsc;
use tokio::time::timeout;

use super::*;
use crate::auth::vault::MemoryStore;
use crate::secrets::SecretString;
use crate::tunnel::endpoint::{Reach, bind};
use crate::tunnel::handshake::tests::{handshake, login_start};
use crate::worlds::fake_service::{self, Fake, ROOM};
use crate::worlds::joining::{self, Route, Visit};
use crate::worlds::service::Token;

const HOST: &str = "11111111-1111-4111-8111-111111111111";
const GUEST: &str = "22222222-2222-4222-8222-222222222222";
const STRANGER: &str = "33333333-3333-4333-8333-333333333333";

struct Fixed(String);

impl Token for Fixed {
    fn token(&self) -> BoxFuture<'_, Result<SecretString, Problem>> {
        let token = self.0.clone();
        Box::pin(async move { Ok(SecretString::new(token)) })
    }
}

fn client(fake: &Fake, account: &str) -> Arc<WorldsClient> {
    Arc::new(
        WorldsClient::new(
            "test",
            &fake.base,
            Arc::new(MemoryStore::default()),
            Arc::new(Fixed(fake_service::token(account))),
            "Test computer",
        )
        .unwrap(),
    )
}

/// A world server that takes connections and remembers what it was told.
struct TestServer {
    port: u16,
    told: Mutex<Vec<String>>,
    running: Mutex<bool>,
    players: Mutex<u32>,
}

impl WorldServer for TestServer {
    fn port(&self) -> u16 {
        self.port
    }
    fn protocol(&self) -> i32 {
        767
    }
    fn running(&self) -> bool {
        *lock(&self.running)
    }
    fn players(&self) -> u32 {
        *lock(&self.players)
    }
    fn set_members(&self, members: &[Nick]) -> LauncherResult<()> {
        let names: Vec<&str> = members.iter().map(Nick::as_str).collect();
        lock(&self.told).push(format!("whitelist {}", names.join(",")));
        Ok(())
    }
    fn kick(&self, nick: &Nick) -> LauncherResult<()> {
        lock(&self.told).push(format!("kick {}", nick.as_str()));
        Ok(())
    }
    fn admitted(&self, account: &str, nick: &Nick) -> LauncherResult<()> {
        lock(&self.told).push(format!("admitted {account} {}", nick.as_str()));
        Ok(())
    }
}

impl TestServer {
    fn told(&self) -> Vec<String> {
        lock(&self.told).clone()
    }
}

async fn test_server() -> (Arc<TestServer>, mpsc::Receiver<TcpStream>) {
    let listener = TcpListener::bind("127.0.0.1:0").await.unwrap();
    let port = listener.local_addr().unwrap().port();
    let (to_test, connections) = mpsc::channel(8);
    tokio::spawn(async move {
        while let Ok((socket, _)) = listener.accept().await {
            if to_test.send(socket).await.is_err() {
                break;
            }
        }
    });
    let server = Arc::new(TestServer {
        port,
        told: Mutex::new(Vec::new()),
        running: Mutex::new(true),
        players: Mutex::new(0),
    });
    (server, connections)
}

fn loopback() -> Rebind {
    Arc::new(|| Box::pin(async { bind(Reach::Loopback, true).await }))
}

fn login(name: &str) -> Vec<u8> {
    let mut wire = handshake(767, 2);
    wire.extend(login_start(name));
    wire
}

async fn eventually(mut check: impl FnMut() -> bool) -> bool {
    for _ in 0..100 {
        if check() {
            return true;
        }
        tokio::time::sleep(Duration::from_millis(100)).await;
    }
    false
}

#[tokio::test(flavor = "multi_thread")]
async fn a_member_plays_in_the_hosts_world_until_removed() {
    let fake =
        fake_service::start(&[(HOST, "Host_Player", true), (GUEST, "Guest_One", false)]).await;
    let (server, mut connections) = test_server().await;
    let host_endpoint = bind(Reach::Loopback, true).await.unwrap();
    let host_address = host_endpoint.addr();
    let host = start(
        client(&fake, HOST),
        Arc::clone(&server) as Arc<dyn WorldServer>,
        host_endpoint,
        loopback(),
        ROOM,
        8,
        None,
        Arc::new(|| {}),
    )
    .await
    .expect("hosting starts");
    assert_eq!(
        server.told().first().map(String::as_str),
        Some("whitelist Guest_One,Host_Player"),
        "the whitelist is set before the first guest"
    );

    // The guest joins and its game logs in through the tunnel.
    let guest_client = client(&fake, GUEST);
    let room = guest_client.room(ROOM).await.unwrap();
    let guest = joining::join(
        Arc::clone(&guest_client),
        bind(Reach::Loopback, false).await.unwrap(),
        Visit {
            room: &room,
            nick: Nick::parse("Guest_One").unwrap(),
            route: Route::Known(host_address.clone()),
            version_name: "1.21.1".into(),
        },
        Arc::new(|| {}),
    )
    .await
    .expect("the guest joins");
    let mut game = TcpStream::connect(("127.0.0.1", guest.port()))
        .await
        .unwrap();
    let sent = login("Guest_One");
    game.write_all(&sent).await.unwrap();
    let mut at_server = timeout(Duration::from_secs(10), connections.recv())
        .await
        .expect("the login reaches the server")
        .unwrap();
    let mut seen = vec![0u8; sent.len()];
    at_server.read_exact(&mut seen).await.unwrap();
    assert_eq!(seen, sent);
    assert!(
        server
            .told()
            .contains(&format!("admitted {GUEST} Guest_One"))
    );
    assert_eq!(host.status().players, ["Guest_One"]);
    assert_eq!(lock(&fake.world).open_admissions(), 1);
    // The renewals tell the service how many play, the host's game included.
    *lock(&server.players) = 2;
    assert!(eventually(|| lock(&fake.world).reported_players == Some(2)).await);
    // On loopback there is no relay: the game data goes directly.
    assert_eq!(guest.path(), Some(joining::PathKind::Direct));

    // A second game of the same account is refused: one place per account.
    let mut second = TcpStream::connect(("127.0.0.1", guest.port()))
        .await
        .unwrap();
    second.write_all(&login("Guest_One")).await.unwrap();
    let mut byte = [0u8; 1];
    let closed = timeout(Duration::from_secs(10), second.read(&mut byte)).await;
    assert!(matches!(closed, Ok(Ok(0) | Err(_))), "{closed:?}");
    assert!(
        timeout(Duration::from_millis(500), connections.recv())
            .await
            .is_err()
    );
    assert_eq!(guest.refused(), Some(Some(Refusal::AlreadyConnected)));

    // The member is removed: the next renewal kicks them and updates the
    // whitelist; their place is released when their connection ends.
    lock(&fake.world).remove(GUEST);
    assert!(eventually(|| server.told().contains(&"kick Guest_One".to_owned())).await);
    assert!(server.told().contains(&"whitelist Host_Player".to_owned()));
    drop(at_server);
    drop(game);
    assert!(eventually(|| lock(&fake.world).open_admissions() == 0).await);
    assert!(eventually(|| host.status().players.is_empty()).await);

    // Stopping ends the session at the service and cuts the tunnel.
    host.stop().await;
    assert!(lock(&fake.world).session.as_ref().unwrap().3);
    assert!(eventually(|| !guest.connected()).await);
    assert_eq!(host.ended(), Some(Ending::Stopped));
}

#[tokio::test(flavor = "multi_thread")]
async fn strangers_get_no_ticket_and_lost_sessions_end_hosting() {
    let fake =
        fake_service::start(&[(HOST, "Host_Player", true), (STRANGER, "Stranger", false)]).await;
    lock(&fake.world).remove(STRANGER);
    let (server, _connections) = test_server().await;
    let host_endpoint = bind(Reach::Loopback, true).await.unwrap();
    let host = start(
        client(&fake, HOST),
        Arc::clone(&server) as Arc<dyn WorldServer>,
        host_endpoint,
        loopback(),
        ROOM,
        8,
        None,
        Arc::new(|| {}),
    )
    .await
    .unwrap();

    let stranger = client(&fake, STRANGER);
    let endpoint = bind(Reach::Loopback, false).await.unwrap();
    let refused = stranger
        .ticket(ROOM, &endpoint.id().to_string())
        .await
        .unwrap_err();
    assert_eq!(refused, Problem::NotMember);

    // Another start of the room elsewhere supersedes this one.
    lock(&fake.world).session.as_mut().unwrap().3 = true;
    assert!(eventually(|| host.ended() == Some(Ending::SessionLost)).await);

    // A server that exits ends hosting too.
    let fake = fake_service::start(&[(HOST, "Host_Player", true)]).await;
    let host = start(
        client(&fake, HOST),
        Arc::clone(&server) as Arc<dyn WorldServer>,
        bind(Reach::Loopback, true).await.unwrap(),
        loopback(),
        ROOM,
        8,
        None,
        Arc::new(|| {}),
    )
    .await
    .unwrap();
    *lock(&server.running) = false;
    assert!(eventually(|| host.ended() == Some(Ending::ServerExited)).await);
    assert!(
        eventually(|| lock(&fake.world).session.as_ref().unwrap().3).await,
        "told the service"
    );
}

#[test]
fn refusals_follow_the_service() {
    assert_eq!(refusal_of(Problem::TicketInvalid), Refusal::TicketInvalid);
    assert_eq!(refusal_of(Problem::Banned), Refusal::NotMember);
    assert_eq!(
        refusal_of(Problem::AlreadyConnected),
        Refusal::AlreadyConnected
    );
    assert_eq!(refusal_of(Problem::RoomFull), Refusal::RoomFull);
    assert_eq!(refusal_of(Problem::SessionEnded), Refusal::WrongSession);
    assert_eq!(refusal_of(Problem::Network), Refusal::Unavailable);
}

#[tokio::test(flavor = "multi_thread")]
async fn a_guest_plays_again_after_quitting_the_game() {
    let fake =
        fake_service::start(&[(HOST, "Host_Player", true), (GUEST, "Guest_One", false)]).await;
    let (server, mut connections) = test_server().await;
    let host_endpoint = bind(Reach::Loopback, true).await.unwrap();
    let host_address = host_endpoint.addr();
    let host = start(
        client(&fake, HOST),
        Arc::clone(&server) as Arc<dyn WorldServer>,
        host_endpoint,
        loopback(),
        ROOM,
        8,
        None,
        Arc::new(|| {}),
    )
    .await
    .expect("hosting starts");
    let guest_client = client(&fake, GUEST);
    let room = guest_client.room(ROOM).await.unwrap();
    let guest = joining::join(
        Arc::clone(&guest_client),
        bind(Reach::Loopback, false).await.unwrap(),
        Visit {
            room: &room,
            nick: Nick::parse("Guest_One").unwrap(),
            route: Route::Known(host_address.clone()),
            version_name: "1.21.1".into(),
        },
        Arc::new(|| {}),
    )
    .await
    .expect("the guest joins");

    for round in 0..3 {
        let mut game = TcpStream::connect(("127.0.0.1", guest.port()))
            .await
            .unwrap();
        let sent = login("Guest_One");
        game.write_all(&sent).await.unwrap();
        let mut at_server = timeout(Duration::from_secs(10), connections.recv())
            .await
            .unwrap_or_else(|_| panic!("login {round} reaches the server"))
            .unwrap();
        let mut seen = vec![0u8; sent.len()];
        at_server.read_exact(&mut seen).await.unwrap();
        assert_eq!(seen, sent);
        // The player quits: the game closes its socket, then the server.
        drop(game);
        drop(at_server);
        assert!(eventually(|| lock(&fake.world).open_admissions() == 0).await);
        assert!(guest.connected(), "round {round}: the tunnel stays");
        // Longer than the tunnel's idle timeout would allow without keep-alive.
        tokio::time::sleep(Duration::from_secs(2)).await;
        assert!(guest.connected(), "round {round}: still there");
        if round == 1 {
            // The network drops: the tunnel comes back by itself.
            guest.cut();
            assert!(eventually(|| !guest.connected()).await);
            assert!(eventually(|| guest.connected() && !guest.reconnecting()).await);
        }
    }
    // The host closes the world: the guest stops trying.
    host.stop().await;
    assert!(eventually(|| guest.gave_up()).await);
    assert!(!guest.connected());
}

#[tokio::test(flavor = "multi_thread")]
async fn a_lapsed_lease_cuts_guests_off_and_opens_a_new_session() {
    let fake =
        fake_service::start(&[(HOST, "Host_Player", true), (GUEST, "Guest_One", false)]).await;
    let (server, _connections) = test_server().await;
    let host_endpoint = bind(Reach::Loopback, true).await.unwrap();
    let host_address = host_endpoint.addr();
    let host = start(
        client(&fake, HOST),
        Arc::clone(&server) as Arc<dyn WorldServer>,
        host_endpoint,
        loopback(),
        ROOM,
        8,
        None,
        Arc::new(|| {}),
    )
    .await
    .unwrap();
    let first = host.status().session;
    let guest_client = client(&fake, GUEST);
    let room = guest_client.room(ROOM).await.unwrap();
    let guest = joining::join(
        Arc::clone(&guest_client),
        bind(Reach::Loopback, false).await.unwrap(),
        Visit {
            room: &room,
            nick: Nick::parse("Guest_One").unwrap(),
            route: Route::Known(host_address.clone()),
            version_name: "1.21.1".into(),
        },
        Arc::new(|| {}),
    )
    .await
    .unwrap();
    assert!(guest.connected());

    // The service stops answering: the lease runs out on this computer's
    // clocks, not on a late answer, and the guest is cut off.
    lock(&fake.world).stalled = true;
    assert!(eventually(|| host.status().reopening).await);
    assert_eq!(host.ended(), None, "the world keeps running");
    assert!(eventually(|| !guest.connected()).await);

    // Back again: a new session on a fresh endpoint, never the lapsed one.
    lock(&fake.world).stalled = false;
    assert!(eventually(|| !host.status().reopening).await);
    assert_eq!(host.ended(), None);
    assert_ne!(host.status().session, first);
    assert_ne!(host.endpoint().id(), host_address.id);
    let live = lock(&fake.world).session.clone().unwrap();
    assert_eq!(live.0, host.status().session);
    assert_eq!(live.2, host.endpoint().id().to_string());

    // A new guest reaches the new endpoint.
    let again = joining::join(
        Arc::clone(&guest_client),
        bind(Reach::Loopback, false).await.unwrap(),
        Visit {
            room: &guest_client.room(ROOM).await.unwrap(),
            nick: Nick::parse("Guest_One").unwrap(),
            route: Route::Known(host.endpoint().addr()),
            version_name: "1.21.1".into(),
        },
        Arc::new(|| {}),
    )
    .await
    .expect("the guest joins the new session");
    assert!(again.connected());
    host.stop().await;
    assert_eq!(host.ended(), Some(Ending::Stopped));
}

#[tokio::test(flavor = "multi_thread")]
async fn hosting_stopped_while_reopening_stays_stopped() {
    let fake = fake_service::start(&[(HOST, "Host_Player", true)]).await;
    let (server, _connections) = test_server().await;
    let host = start(
        client(&fake, HOST),
        Arc::clone(&server) as Arc<dyn WorldServer>,
        bind(Reach::Loopback, true).await.unwrap(),
        loopback(),
        ROOM,
        8,
        None,
        Arc::new(|| {}),
    )
    .await
    .unwrap();
    lock(&fake.world).stalled = true;
    assert!(eventually(|| host.status().reopening).await);
    host.stop().await;
    assert_eq!(host.ended(), Some(Ending::Stopped));
    lock(&fake.world).stalled = false;
    tokio::time::sleep(Duration::from_secs(2)).await;
    assert_eq!(
        host.ended(),
        Some(Ending::Stopped),
        "no session after a stop"
    );
    assert!(!host.status().reopening);

    // A server that exits while the service is away ends hosting too.
    let fake = fake_service::start(&[(HOST, "Host_Player", true)]).await;
    let host = start(
        client(&fake, HOST),
        Arc::clone(&server) as Arc<dyn WorldServer>,
        bind(Reach::Loopback, true).await.unwrap(),
        loopback(),
        ROOM,
        8,
        None,
        Arc::new(|| {}),
    )
    .await
    .unwrap();
    lock(&fake.world).stalled = true;
    assert!(eventually(|| host.status().reopening).await);
    *lock(&server.running) = false;
    assert!(eventually(|| host.ended() == Some(Ending::ServerExited)).await);
}

#[test]
fn a_lease_counts_on_both_clocks_and_fails_closed() {
    assert!(Granted::now().left() > LEASE / 2);
    // The computer slept: the wall clock moved on, the monotonic one did not.
    let slept = Granted {
        at: Instant::now(),
        wall: SystemTime::now() - Duration::from_secs(3600),
    };
    assert_eq!(slept.left(), Duration::ZERO);
    // The clock was turned back.
    let turned = Granted {
        at: Instant::now(),
        wall: SystemTime::now() + Duration::from_secs(60),
    };
    assert_eq!(turned.left(), Duration::ZERO);
    // A small correction is not a lapse.
    let corrected = Granted {
        at: Instant::now(),
        wall: SystemTime::now() + Duration::from_millis(500),
    };
    assert!(corrected.left() > Duration::ZERO);
}
