//! A small stand-in for worlds.lapi.sh in tests: one room, its members, one
//! session at a time, tickets, admissions and the relay. It checks the DPoP
//! proofs as the real service does (key, method, URL, token, single use) and
//! refuses calls from devices that were not registered.

#![allow(clippy::unwrap_used)]

use std::collections::{HashMap, HashSet};
use std::sync::{Arc, Mutex};
use std::time::Duration;

use base64::Engine;
use base64::engine::general_purpose::URL_SAFE_NO_PAD as B64;
use serde_json::{Value, json};
use sha2::{Digest, Sha256};
use tokio::io::{AsyncReadExt, AsyncWriteExt};

use crate::sync::lock;

pub const ROOM: &str = "6f1d3b8e-2c4a-4f7e-9b1a-0c2d3e4f5a6b";

#[derive(Debug, Clone)]
pub struct Account {
    pub id: String,
    pub nick: String,
    pub active: bool,
    pub owner: bool,
}

#[derive(Debug, Clone)]
pub struct Admission {
    pub id: String,
    pub account: String,
    pub released: bool,
    pub revoked: bool,
}

#[derive(Debug, Default)]
pub struct World {
    /// Token to account.
    pub accounts: HashMap<String, Account>,
    /// Device key (base64url) to account.
    pub devices: HashMap<String, String>,
    /// Registered devices: id, account, name.
    pub device_list: Vec<(String, String, String)>,
    pub session: Option<(String, i64, String, bool)>,
    /// Ticket to (account, guest endpoint, used).
    pub tickets: HashMap<String, (String, String, bool)>,
    pub admissions: Vec<Admission>,
    pub enrolled: Vec<String>,
    /// The player count the last renewal told.
    pub reported_players: Option<u64>,
    /// The service is out of reach: renewals get no answer, other calls fail.
    pub stalled: bool,
    pub calls: Vec<String>,
    jtis: HashSet<String>,
    next: u64,
}

impl World {
    fn id(&mut self) -> String {
        self.next += 1;
        format!("00000000-0000-4000-8000-{:012}", self.next)
    }

    fn member(&self, account: &str) -> Option<&Account> {
        self.accounts.values().find(|a| a.id == account)
    }

    /// The member leaves or is removed: their open admissions are revoked.
    pub fn remove(&mut self, account: &str) {
        for a in self.accounts.values_mut() {
            if a.id == account {
                a.active = false;
            }
        }
        for admission in &mut self.admissions {
            if admission.account == account && !admission.released {
                admission.revoked = true;
            }
        }
    }

    pub fn open_admissions(&self) -> usize {
        self.admissions.iter().filter(|a| !a.released).count()
    }
}

/// A token the client can read its account from.
pub fn token(account: &str) -> String {
    format!(
        "e30.{}.c2ln",
        B64.encode(json!({ "sub": account }).to_string())
    )
}

pub struct Fake {
    pub base: String,
    pub world: Arc<Mutex<World>>,
}

/// Starts the fake with `accounts` as (account id, nick, owner).
pub async fn start(accounts: &[(&str, &str, bool)]) -> Fake {
    let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
    let base = format!("http://{}", listener.local_addr().unwrap());
    let world = Arc::new(Mutex::new(World::default()));
    for (id, nick, owner) in accounts {
        lock(&world).accounts.insert(
            token(id),
            Account {
                id: (*id).to_owned(),
                nick: (*nick).to_owned(),
                active: true,
                owner: *owner,
            },
        );
    }
    let (shared, url) = (Arc::clone(&world), base.clone());
    tokio::spawn(async move {
        while let Ok((mut socket, _)) = listener.accept().await {
            let (world, base) = (Arc::clone(&shared), url.clone());
            tokio::spawn(async move {
                let mut request = Vec::new();
                let mut buffer = [0u8; 8192];
                let (head, body) = loop {
                    let n = socket.read(&mut buffer).await.unwrap_or(0);
                    if n == 0 {
                        return;
                    }
                    request.extend_from_slice(&buffer[..n]);
                    let text = String::from_utf8_lossy(&request).into_owned();
                    if let Some((head, body)) = text.split_once("\r\n\r\n") {
                        let length = header(head, "content-length")
                            .and_then(|v| v.parse::<usize>().ok())
                            .unwrap_or(0);
                        if body.len() >= length {
                            break (head.to_owned(), body.to_owned());
                        }
                    }
                };
                if lock(&world).stalled {
                    if head.contains("/renew ") {
                        tokio::time::sleep(Duration::from_secs(5)).await;
                    }
                    return;
                }
                let (status, answer) = handle(&world, &base, &head, &body);
                let reply = format!(
                    "HTTP/1.1 {status} X\r\ncontent-type: application/json\r\ncontent-length: {}\r\nconnection: close\r\n\r\n{answer}",
                    answer.len()
                );
                let _ = socket.write_all(reply.as_bytes()).await;
            });
        }
    });
    Fake { base, world }
}

fn header(head: &str, name: &str) -> Option<String> {
    head.lines().skip(1).find_map(|line| {
        let (key, value) = line.split_once(':')?;
        key.trim()
            .eq_ignore_ascii_case(name)
            .then(|| value.trim().to_owned())
    })
}

fn refuse(code: &str) -> (u16, String) {
    let status = match code {
        "unauthorized" | "invalid_proof" => 401,
        "forbidden" | "device_unknown" | "ticket_invalid" | "not_member" => 403,
        "not_found" => 404,
        "bad_request" => 400,
        _ => 409,
    };
    (status, json!({ "error": code }).to_string())
}

/// The device key of a valid proof for this request, as the real service
/// checks it.
fn proof_key(
    world: &mut World,
    proof: &str,
    method: &str,
    url: &str,
    token: &str,
) -> Option<String> {
    let mut parts = proof.split('.');
    let (head, body, signature) = (parts.next()?, parts.next()?, parts.next()?);
    let header: Value = serde_json::from_slice(&B64.decode(head).ok()?).ok()?;
    let payload: Value = serde_json::from_slice(&B64.decode(body).ok()?).ok()?;
    if header["typ"] != "dpop+jwt" || header["alg"] != "EdDSA" || header["jwk"]["crv"] != "Ed25519"
    {
        return None;
    }
    let x = header["jwk"]["x"].as_str()?;
    let key_bytes: [u8; 32] = B64.decode(x).ok()?.try_into().ok()?;
    let key = iroh::PublicKey::from_bytes(&key_bytes).ok()?;
    let signature: [u8; 64] = B64.decode(signature).ok()?.try_into().ok()?;
    key.verify(
        format!("{head}.{body}").as_bytes(),
        &iroh::Signature::from_bytes(&signature),
    )
    .ok()?;
    let now = chrono::Utc::now().timestamp();
    let fresh = (now - payload["iat"].as_i64()?).abs() <= 60;
    let bound = payload["htm"] == method
        && payload["htu"] == url
        && payload["ath"] == B64.encode(Sha256::digest(token.as_bytes()));
    let jti = payload["jti"].as_str()?.to_owned();
    (fresh && bound && world.jtis.insert(jti)).then(|| x.to_owned())
}

fn handle(world: &Mutex<World>, base: &str, head: &str, body: &str) -> (u16, String) {
    let mut world = lock(world);
    let mut first = head.lines().next().unwrap_or("").split(' ');
    let (method, path) = (first.next().unwrap_or(""), first.next().unwrap_or(""));
    world.calls.push(format!("{method} {path}"));
    let token = header(head, "authorization")
        .and_then(|v| v.strip_prefix("Bearer ").map(str::to_owned))
        .unwrap_or_default();
    let Some(me) = world.accounts.get(&token).cloned() else {
        return refuse("unauthorized");
    };
    let body: Value = serde_json::from_str(body).unwrap_or(Value::Null);
    let segments: Vec<&str> = path.trim_start_matches('/').split('/').collect();
    let device = header(head, "dpop")
        .map(|proof| proof_key(&mut world, &proof, method, &format!("{base}{path}"), &token));
    let registered = |world: &World| match &device {
        Some(Some(key)) => world.devices.get(key) == Some(&me.id),
        _ => false,
    };
    match (method, segments.as_slice()) {
        ("POST", ["v1", "devices"]) => {
            let Some(Some(key)) = device else {
                return refuse("invalid_proof");
            };
            if world.devices.get(&key).is_some_and(|owner| *owner != me.id) {
                return refuse("device_key_taken");
            }
            world.devices.insert(key, me.id.clone());
            let id = world.id();
            let name = body["name"].as_str().unwrap_or_default().to_owned();
            world
                .device_list
                .push((id.clone(), me.id.clone(), name.clone()));
            (
                200,
                json!({ "id": id, "name": name, "created": 1 }).to_string(),
            )
        }
        ("GET", ["v1", "export"]) => {
            let devices: Vec<Value> = world
                .device_list
                .iter()
                .filter(|(_, account, _)| *account == me.id)
                .map(|(id, _, name)| json!({ "id": id, "name": name }))
                .collect();
            (200, json!({ "devices": devices }).to_string())
        }
        ("GET", ["v1", "devices"]) => {
            let mine: Vec<Value> = world
                .device_list
                .iter()
                .filter(|(_, account, _)| *account == me.id)
                .map(|(id, _, name)| json!({ "id": id, "name": name, "created": 1 }))
                .collect();
            (200, Value::from(mine).to_string())
        }
        ("DELETE", ["v1", "devices", device]) => {
            let before = world.device_list.len();
            world
                .device_list
                .retain(|(id, account, _)| !(id == device && *account == me.id));
            if world.device_list.len() == before {
                return refuse("not_found");
            }
            (200, json!({ "ok": true }).to_string())
        }
        ("GET", ["v1", "rooms", room]) if *room == ROOM => {
            if !me.active {
                return refuse("not_found");
            }
            let hosting = world.session.as_ref().is_some_and(|s| !s.3);
            let online = world.open_admissions();
            let owner = world.accounts.values().find(|a| a.owner).cloned().unwrap();
            (
                200,
                json!({
                    "id": ROOM, "name": "Our world", "description": "", "minecraft": "1.21.1",
                    "revision": 1, "owner": owner.id, "ownerNick": owner.nick,
                    "role": if me.owner { "owner" } else { "member" },
                    "members": world.accounts.len(), "hosting": hosting, "online": online,
                    "maxPlayers": 8, "created": 1, "updated": 1,
                })
                .to_string(),
            )
        }
        (_, ["v1", "rooms", ..] | ["v1", "sessions", ..])
            if device.is_some() && !registered(&world) =>
        {
            match device {
                Some(None) => refuse("invalid_proof"),
                _ => refuse("device_unknown"),
            }
        }
        ("POST", ["v1", "rooms", room, "sessions"]) if *room == ROOM && device.is_some() => {
            if !me.owner {
                return refuse("forbidden");
            }
            let epoch = world.session.as_ref().map_or(1, |s| s.1 + 1);
            let id = world.id();
            let endpoint = body["endpoint"].as_str().unwrap_or("").to_owned();
            world.session = Some((id.clone(), epoch, endpoint.clone(), false));
            (
                200,
                json!({ "id": id, "room": ROOM, "epoch": epoch, "endpoint": endpoint,
                        "maxPlayers": body["maxPlayers"], "leaseUntil": 30 })
                .to_string(),
            )
        }
        ("POST", ["v1", "sessions", session, action]) if device.is_some() => {
            let live = world
                .session
                .as_ref()
                .is_some_and(|s| s.0 == *session && !s.3);
            if !live {
                return refuse("session_ended");
            }
            match *action {
                "renew" => {
                    world.reported_players = body["players"].as_u64();
                    let mut members: Vec<String> = world
                        .accounts
                        .values()
                        .filter(|a| a.active)
                        .map(|a| a.nick.clone())
                        .collect();
                    members.sort();
                    let revoked: Vec<String> = world
                        .admissions
                        .iter()
                        .filter(|a| a.revoked && !a.released)
                        .map(|a| a.id.clone())
                        .collect();
                    (
                        200,
                        json!({ "leaseUntil": 30, "members": members, "revoked": revoked })
                            .to_string(),
                    )
                }
                "stop" => {
                    if let Some(s) = world.session.as_mut() {
                        s.3 = true;
                    }
                    (200, json!({ "ok": true }).to_string())
                }
                "admissions" => {
                    let ticket = body["ticket"].as_str().unwrap_or("").to_owned();
                    let peer = body["peer"].as_str().unwrap_or("").to_owned();
                    let Some((account, endpoint, used)) = world.tickets.get(&ticket).cloned()
                    else {
                        return refuse("ticket_invalid");
                    };
                    world
                        .tickets
                        .insert(ticket, (account.clone(), endpoint.clone(), true));
                    if used || endpoint != peer {
                        return refuse("ticket_invalid");
                    }
                    let Some(member) = world.member(&account).filter(|m| m.active).cloned() else {
                        return refuse("not_member");
                    };
                    if world
                        .admissions
                        .iter()
                        .any(|a| a.account == account && !a.released)
                    {
                        return refuse("already_connected");
                    }
                    let id = world.id();
                    world.admissions.push(Admission {
                        id: id.clone(),
                        account: account.clone(),
                        released: false,
                        revoked: false,
                    });
                    (
                        200,
                        json!({ "id": id, "account": account, "nick": member.nick }).to_string(),
                    )
                }
                _ => refuse("not_found"),
            }
        }
        (
            "POST",
            [
                "v1",
                "sessions",
                session,
                "admissions",
                admission,
                "release",
            ],
        ) if device.is_some() => {
            let live = world.session.as_ref().is_some_and(|s| s.0 == *session);
            match world.admissions.iter_mut().find(|a| a.id == *admission) {
                Some(found) if live => {
                    found.released = true;
                    (200, json!({ "ok": true }).to_string())
                }
                _ => refuse("not_found"),
            }
        }
        ("POST", ["v1", "rooms", room, "tickets"]) if *room == ROOM && device.is_some() => {
            if !me.active {
                return refuse("not_member");
            }
            let Some((session, epoch, host, false)) = world.session.clone() else {
                return refuse("not_hosting");
            };
            let ticket = format!("ticket-{}", world.id());
            let endpoint = body["endpoint"].as_str().unwrap_or("").to_owned();
            world
                .tickets
                .insert(ticket.clone(), (me.id.clone(), endpoint, false));
            (
                200,
                json!({ "ticket": ticket, "session": session, "hostEpoch": epoch.to_string(),
                        "hostEndpoint": host, "expires": 60 })
                .to_string(),
            )
        }
        ("POST", ["v1", "rooms", room, "relay"]) if *room == ROOM && device.is_some() => {
            if !me.active {
                return refuse("not_member");
            }
            let endpoint = body["endpoint"].as_str().unwrap_or("").to_owned();
            world.enrolled.push(endpoint);
            (200, json!({ "ttl": 30 }).to_string())
        }
        _ => refuse("not_found"),
    }
}
