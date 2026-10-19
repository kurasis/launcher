//! The tunnel end to end over loopback iroh endpoints: a game client against
//! the guest listener, the host side, and a stand-in world server
//! (acceptance A11, A14, A53, A86).

use std::sync::atomic::{AtomicU32, Ordering};
use std::sync::{Arc, Mutex};
use std::time::{Duration, Instant};

use iroh::EndpointId;
use iroh::endpoint::Connection;
use tokio::io::{AsyncReadExt, AsyncWriteExt};
use tokio::net::{TcpListener, TcpStream};
use tokio::sync::mpsc;
use tokio::time::timeout;

use super::endpoint::{Reach, bind};
use super::frame::{
    ALPN, AdmissionRequest, AdmissionResponse, PROTOCOL_VERSION, Refusal, Service, read_frame,
    write_frame,
};
use super::guest::{self, Grant, GuestListener, RoomStatus, StatusSnapshot, Target, Tickets};
use super::handshake::tests::{handshake, login_start};
use super::host::{self, Admit, Admitted, TunnelHost};
use crate::error::LauncherError;
use crate::install::service::BoxFuture;
use crate::worlds::identity::Nick;
use crate::worlds::protocol::{read_packet, read_string, write_packet};

/// The tests share the launcher-wide limits, so they run one at a time.
static SERIAL: tokio::sync::Mutex<()> = tokio::sync::Mutex::const_new(());

const GOOD: &str = "good-ticket";

struct TestAdmit {
    nick: Nick,
    next: AtomicU32,
    released: Mutex<Vec<String>>,
}

impl Admit for TestAdmit {
    fn admit<'a>(
        &'a self,
        _peer: EndpointId,
        request: &'a AdmissionRequest,
    ) -> BoxFuture<'a, Result<Admitted, Refusal>> {
        Box::pin(async move {
            if request.ticket != GOOD {
                return Err(Refusal::TicketInvalid);
            }
            Ok(Admitted {
                connection_id: format!("c{}", self.next.fetch_add(1, Ordering::SeqCst)),
                nick: self.nick.clone(),
            })
        })
    }

    fn released(&self, connection_id: &str) {
        self.released.lock().unwrap().push(connection_id.to_owned());
    }
}

struct FixedTickets(&'static str);

impl Tickets for FixedTickets {
    fn ticket(&self) -> BoxFuture<'_, Result<Grant, LauncherError>> {
        Box::pin(async move {
            Ok(Grant {
                ticket: self.0.to_owned(),
                session_id: "s-1".into(),
                host_epoch: "1".into(),
            })
        })
    }
}

#[derive(Default)]
struct TestStatus {
    refusals: Mutex<Vec<Option<Refusal>>>,
}

impl RoomStatus for TestStatus {
    fn snapshot(&self) -> Option<StatusSnapshot> {
        Some(StatusSnapshot {
            observed_at: Instant::now(),
            name: "Our \"world\"".into(),
            online: 1,
            max: 8,
        })
    }

    fn refused(&self, refusal: Option<Refusal>) {
        self.refusals.lock().unwrap().push(refusal);
    }
}

struct Setup {
    _host: TunnelHost,
    listener: GuestListener,
    connection: Connection,
    servers: mpsc::Receiver<TcpStream>,
    admit: Arc<TestAdmit>,
    status: Arc<TestStatus>,
}

async fn setup(ticket: &'static str) -> Setup {
    let server = TcpListener::bind("127.0.0.1:0").await.unwrap();
    let server_port = server.local_addr().unwrap().port();
    let (to_test, servers) = mpsc::channel(16);
    tokio::spawn(async move {
        while let Ok((socket, _)) = server.accept().await {
            if to_test.send(socket).await.is_err() {
                break;
            }
        }
    });
    let host_endpoint = bind(Reach::Loopback, true).await.unwrap();
    let address = host_endpoint.addr();
    let admit = Arc::new(TestAdmit {
        nick: Nick::parse("Notch").unwrap(),
        next: AtomicU32::new(1),
        released: Mutex::new(Vec::new()),
    });
    let host = host::serve(
        host_endpoint,
        Arc::clone(&admit) as Arc<dyn Admit>,
        server_port,
        767,
    );
    let guest_endpoint = bind(Reach::Loopback, false).await.unwrap();
    let connection = guest_endpoint.connect(address, ALPN).await.unwrap();
    let status = Arc::new(TestStatus::default());
    let listener = guest::listen(Target {
        connection: connection.clone(),
        nick: Nick::parse("Notch").unwrap(),
        protocol: 767,
        version_name: "1.21.1",
        tickets: Arc::new(FixedTickets(ticket)),
        status: Arc::clone(&status) as Arc<dyn RoomStatus>,
    })
    .await
    .unwrap();
    // The guest endpoint must outlive the test's connections.
    std::mem::forget(guest_endpoint);
    Setup {
        _host: host,
        listener,
        connection,
        servers,
        admit,
        status,
    }
}

fn login(name: &str) -> Vec<u8> {
    let mut wire = handshake(767, 2);
    wire.extend(login_start(name));
    wire
}

async fn client(setup: &Setup) -> TcpStream {
    TcpStream::connect(("127.0.0.1", setup.listener.port()))
        .await
        .unwrap()
}

async fn no_server_connection(setup: &mut Setup) -> bool {
    timeout(Duration::from_millis(1500), setup.servers.recv())
        .await
        .is_err()
}

#[tokio::test]
async fn a_login_reaches_the_server_with_exactly_its_bytes() {
    let _serial = SERIAL.lock().await;
    let mut setup = setup(GOOD).await;
    let mut game = client(&setup).await;
    let mut sent = login("Notch");
    sent.extend_from_slice(b"after-login");
    // Fragmented: the classifier sees partial packets first.
    for part in sent.chunks(3) {
        game.write_all(part).await.unwrap();
        tokio::time::sleep(Duration::from_millis(5)).await;
    }
    let mut server = timeout(Duration::from_secs(10), setup.servers.recv())
        .await
        .unwrap()
        .unwrap();
    let mut received = vec![0u8; sent.len()];
    server.read_exact(&mut received).await.unwrap();
    assert_eq!(received, sent, "ordered, once, unchanged");

    server.write_all(b"from-server").await.unwrap();
    let mut reply = [0u8; 11];
    game.read_exact(&mut reply).await.unwrap();
    assert_eq!(&reply, b"from-server");

    // Half-close from the game: the server sees end of input and may still answer.
    game.shutdown().await.unwrap();
    let mut rest = Vec::new();
    timeout(Duration::from_secs(5), server.read_to_end(&mut rest))
        .await
        .unwrap()
        .unwrap();
    assert!(rest.is_empty());
    server.write_all(b"late").await.unwrap();
    server.shutdown().await.unwrap();
    let mut tail = Vec::new();
    timeout(Duration::from_secs(5), game.read_to_end(&mut tail))
        .await
        .unwrap()
        .unwrap();
    assert_eq!(tail, b"late");
    tokio::time::sleep(Duration::from_millis(200)).await;
    assert_eq!(*setup.admit.released.lock().unwrap(), ["c1"]);
}

#[tokio::test]
async fn traffic_in_both_directions_at_once() {
    let _serial = SERIAL.lock().await;
    let mut setup = setup(GOOD).await;
    let mut game = client(&setup).await;
    let prefix = login("Notch");
    game.write_all(&prefix).await.unwrap();
    let mut server = setup.servers.recv().await.unwrap();
    let mut seen = vec![0u8; prefix.len()];
    server.read_exact(&mut seen).await.unwrap();

    let size = 4 * 1024 * 1024;
    let pattern = |seed: u8| -> Vec<u8> {
        (0..size)
            .map(|i: usize| (i as u8).wrapping_mul(31).wrapping_add(seed))
            .collect()
    };
    let (up, down) = (pattern(1), pattern(2));
    let (mut game_read, mut game_write) = game.into_split();
    let (mut server_read, mut server_write) = server.into_split();
    let (up_sent, down_sent) = (up.clone(), down.clone());
    let writers = tokio::spawn(async move {
        let a = async {
            game_write.write_all(&up_sent).await.unwrap();
            game_write.shutdown().await.unwrap();
        };
        let b = async {
            server_write.write_all(&down_sent).await.unwrap();
            server_write.shutdown().await.unwrap();
        };
        tokio::join!(a, b);
    });
    let mut at_server = Vec::new();
    let mut at_game = Vec::new();
    let (r1, r2) = tokio::join!(
        server_read.read_to_end(&mut at_server),
        game_read.read_to_end(&mut at_game)
    );
    r1.unwrap();
    r2.unwrap();
    writers.await.unwrap();
    assert!(at_server == up, "game to server intact");
    assert!(at_game == down, "server to game intact");
}

#[tokio::test]
async fn a_reset_on_one_side_reaches_the_other() {
    let _serial = SERIAL.lock().await;
    let mut setup = setup(GOOD).await;
    let mut game = client(&setup).await;
    let prefix = login("Notch");
    game.write_all(&prefix).await.unwrap();
    let mut server = setup.servers.recv().await.unwrap();
    let mut seen = vec![0u8; prefix.len()];
    server.read_exact(&mut seen).await.unwrap();
    socket2::SockRef::from(&server)
        .set_linger(Some(Duration::ZERO))
        .unwrap();
    drop(server);
    let mut buffer = [0u8; 16];
    let result = timeout(Duration::from_secs(5), game.read(&mut buffer))
        .await
        .unwrap();
    assert!(
        matches!(result, Err(_) | Ok(0)),
        "the game's connection ends: {result:?}"
    );
}

#[tokio::test]
async fn a_wrong_ticket_never_reaches_the_server() {
    let _serial = SERIAL.lock().await;
    let mut setup = setup("forged").await;
    let mut game = client(&setup).await;
    game.write_all(&login("Notch")).await.unwrap();
    let mut buffer = [0u8; 16];
    let read = timeout(Duration::from_secs(5), game.read(&mut buffer))
        .await
        .unwrap();
    assert!(matches!(read, Ok(0) | Err(_)));
    assert!(no_server_connection(&mut setup).await);
    assert_eq!(
        *setup.status.refusals.lock().unwrap(),
        [Some(Refusal::TicketInvalid)]
    );
}

#[tokio::test]
async fn another_name_never_reaches_the_server() {
    let _serial = SERIAL.lock().await;
    let mut setup = setup(GOOD).await;
    // Through the listener: refused locally before any ticket.
    let mut game = client(&setup).await;
    game.write_all(&login("Mallory")).await.unwrap();
    let mut buffer = [0u8; 16];
    let read = timeout(Duration::from_secs(5), game.read(&mut buffer))
        .await
        .unwrap();
    assert!(matches!(read, Ok(0) | Err(_)));
    // Around the listener: a valid ticket, then another login name.
    let (mut send, mut recv) = setup.connection.open_bi().await.unwrap();
    write_frame(&mut send, &request(GOOD, Service::Gameplay))
        .await
        .unwrap();
    let response: AdmissionResponse = read_frame(&mut recv).await.unwrap();
    assert!(response.accepted);
    send.write_all(&login("Mallory")).await.unwrap();
    let mut rest = [0u8; 8];
    let closed = timeout(Duration::from_secs(5), recv.read(&mut rest))
        .await
        .unwrap();
    assert!(matches!(closed, Err(_) | Ok(None)));
    assert!(no_server_connection(&mut setup).await);
    tokio::time::sleep(Duration::from_millis(100)).await;
    assert_eq!(*setup.admit.released.lock().unwrap(), ["c1"]);
}

fn request(ticket: &str, service: Service) -> AdmissionRequest {
    AdmissionRequest {
        protocol_version: PROTOCOL_VERSION,
        request_id: "r-1".into(),
        ticket: ticket.into(),
        session_id: "s-1".into(),
        expected_host_epoch: "1".into(),
        service,
    }
}

#[tokio::test]
async fn status_is_answered_locally_and_never_forwarded() {
    let _serial = SERIAL.lock().await;
    let mut setup = setup(GOOD).await;
    let mut game = client(&setup).await;
    let mut wire = handshake(767, 1);
    write_packet(&mut wire, 0x00, &[]).unwrap();
    game.write_all(&wire).await.unwrap();
    let mut buffer = vec![0u8; 4096];
    let (id, body) = timeout(Duration::from_secs(5), async {
        let mut collected = Vec::new();
        loop {
            let read = game.read(&mut buffer).await.unwrap();
            collected.extend_from_slice(&buffer[..read]);
            if let Ok(packet) = read_packet(&mut &collected[..], 64 * 1024) {
                return packet;
            }
        }
    })
    .await
    .unwrap();
    assert_eq!(id, 0);
    let (json, _) = read_string(&body, 64 * 1024).unwrap();
    let value: serde_json::Value = serde_json::from_str(&json).unwrap();
    assert_eq!(value["version"]["protocol"], 767);
    assert_eq!(value["players"]["online"], 1);
    assert_eq!(value["description"]["text"], "Our \"world\"");
    let mut ping = Vec::new();
    write_packet(&mut ping, 0x01, &42i64.to_be_bytes()).unwrap();
    game.write_all(&ping).await.unwrap();
    let mut pong = [0u8; 10];
    game.read_exact(&mut pong).await.unwrap();
    assert_eq!(&pong[..2], &[9, 1]);
    assert_eq!(&pong[2..], &42i64.to_be_bytes());
    assert!(no_server_connection(&mut setup).await);
    assert!(
        setup.admit.released.lock().unwrap().is_empty(),
        "no ticket used"
    );
}

#[tokio::test]
async fn gameplay_bytes_declaring_status_are_rejected() {
    // A53: a raw stream naming another service, and an admitted stream that
    // then sends a status handshake.
    let _serial = SERIAL.lock().await;
    let mut setup = setup(GOOD).await;
    let (mut send, mut recv) = setup.connection.open_bi().await.unwrap();
    let json = serde_json::to_string(&request(GOOD, Service::Gameplay))
        .unwrap()
        .replace("\"gameplay\"", "\"status\"");
    let mut frame = (json.len() as u32).to_be_bytes().to_vec();
    frame.extend_from_slice(json.as_bytes());
    send.write_all(&frame).await.unwrap();
    let response: AdmissionResponse = read_frame(&mut recv).await.unwrap();
    assert!(!response.accepted);
    assert_eq!(response.refusal, Some(Refusal::BadFrame));

    let (mut send, mut recv) = setup.connection.open_bi().await.unwrap();
    write_frame(&mut send, &request(GOOD, Service::Gameplay))
        .await
        .unwrap();
    let response: AdmissionResponse = read_frame(&mut recv).await.unwrap();
    assert!(response.accepted);
    let mut status = handshake(767, 1);
    write_packet(&mut status, 0x00, &[]).unwrap();
    send.write_all(&status).await.unwrap();
    let mut rest = [0u8; 8];
    let closed = timeout(Duration::from_secs(5), recv.read(&mut rest))
        .await
        .unwrap();
    assert!(matches!(closed, Err(_) | Ok(None)));
    assert!(no_server_connection(&mut setup).await);
}

#[tokio::test]
async fn unadmitted_streams_are_bounded() {
    // A14: a fifth waiting stream on one connection is reset at once.
    let _serial = SERIAL.lock().await;
    let setup = setup(GOOD).await;
    let mut waiting = Vec::new();
    for _ in 0..4 {
        let (mut send, recv) = setup.connection.open_bi().await.unwrap();
        send.write_all(&[0, 0]).await.unwrap();
        waiting.push((send, recv));
    }
    tokio::time::sleep(Duration::from_millis(300)).await;
    let (mut send, mut recv) = setup.connection.open_bi().await.unwrap();
    send.write_all(&[0, 0]).await.unwrap();
    let mut buffer = [0u8; 8];
    let refused = timeout(Duration::from_secs(3), recv.read(&mut buffer)).await;
    assert!(
        matches!(refused, Ok(Err(_))),
        "reset without waiting: {refused:?}"
    );
    drop(waiting);
}

#[tokio::test]
async fn a_stalled_server_stops_the_game_instead_of_queueing() {
    // A14: backpressure; forwarding memory stays within the stream's buffers.
    let _serial = SERIAL.lock().await;
    let mut setup = setup(GOOD).await;
    let before = super::bridge::available();
    let mut game = client(&setup).await;
    game.write_all(&login("Notch")).await.unwrap();
    let server = setup.servers.recv().await.unwrap();
    assert_eq!(
        super::bridge::available(),
        before - 4,
        "two streams' buffers"
    );
    let flood = vec![0u8; 64 * 1024 * 1024];
    let stalled = timeout(Duration::from_secs(3), game.write_all(&flood)).await;
    assert!(stalled.is_err(), "the writer is held back");
    drop(game);
    drop(server);
    tokio::time::sleep(Duration::from_millis(500)).await;
    assert_eq!(super::bridge::available(), before, "buffers returned");
}

#[tokio::test]
async fn local_pending_connections_are_bounded() {
    // A86: a ninth silent local connection is closed at once.
    let _serial = SERIAL.lock().await;
    let setup = setup(GOOD).await;
    let mut silent = Vec::new();
    for _ in 0..guest::PENDING_PER_LISTENER {
        silent.push(client(&setup).await);
    }
    tokio::time::sleep(Duration::from_millis(200)).await;
    let mut extra = client(&setup).await;
    let mut buffer = [0u8; 4];
    let read = timeout(Duration::from_secs(2), extra.read(&mut buffer)).await;
    assert!(matches!(read, Ok(Ok(0)) | Ok(Err(_))), "closed at once");
    // The others are closed by the classification deadline.
    let read = timeout(Duration::from_secs(8), silent[0].read(&mut buffer)).await;
    assert!(matches!(read, Ok(Ok(0)) | Ok(Err(_))));
}
