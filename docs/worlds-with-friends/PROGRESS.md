# Progress

States: `not_started`, `in_progress`, `blocked`, `done`. A phase whose code is finished but
whose live check waits for an outside input is `blocked`, with the input named. Evidence says
whether it is a unit test, a local run, CI or a live run with real accounts and networks.

| Phase                         | Status      | Evidence                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                         | Blocker / notes                                                                                                                                                                 |
| ----------------------------- | ----------- | ------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------ | ------------------------------------------------------------------------------------------------------------------------------------------------------------------------------- |
| W1 Foundation                 | done        | This directory and [D-068](../DECISIONS.md#d-068--worlds-with-friends); documents only, no code                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                  | —                                                                                                                                                                               |
| W2 Game and server together   | done        | Unit tests in `src-tauri/src/state/activity.rs` for every pair of activities and a 200-round race of a server against an install                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                 | The permit has no caller until W3 starts a server                                                                                                                               |
| W3 Server on the own computer | done        | Unit tests in `src-tauri/src/worlds/`; `tests/live_server.rs` run locally and in the CI job `World server (linux-x64)`: the real 1.21.1 server prepared, ready, RCON checked, stopped clean, then force stopped and recorded unclean                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                             | Windows and macOS runs of the server come with W10 and W11                                                                                                                      |
| W4 Tunnel                     | done        | iroh 1.3.0 pinned; 16 tests in `src-tauri/src/tunnel/` over loopback endpoints: exact bytes with fragmentation, both directions at once, half-close, reset, wrong ticket, wrong name, local status, status declared as gameplay, bounded unadmitted streams, backpressure, bounded local sockets                                                                                                                                                                                                                                                                                                                                                                                                                                                 | Two machines on different networks: W7 and W10                                                                                                                                  |
| W5 worlds.lapi.sh             | done        | 19 tests in `services/worlds`: DPoP proofs (replay, wrong URL, stale, foreign key), audience and nick, the permission table, invite bounds, last use, expiry, bans, two hosts racing, ticket replay and stolen tickets, one slot per account, a full room, a late renewal, a removed member, a restored database; `audience worlds` in oauth                                                                                                                                                                                                                                                                                                                                                                                                     | Owner: DNS record and certificate for worlds.lapi.sh, `deploy/bootstrap.sh` once                                                                                                |
| W6 Relay                      | done        | `services/relay`: 5 tests against a real iroh-relay 1.3.0 on loopback (a listed key connects, an unlisted one is refused, a busy key that drops off the list is closed, a record not renewed closes the connection, the list needs the token, a failed read keeps records until they run out, per-account limit); `services/worlds`: allow list test (hosts, enrolled members, removal, 30 s expiry, token)                                                                                                                                                                                                                                                                                                                                      | Owner: a VPS (x86-64 Linux), DNS record relay.lapi.sh (not proxied), ports 80/tcp, 443/tcp, 7842/udp, `RELAY_SSH_*` and `RELAY_TOKEN` secrets, `deploy/relay/bootstrap.sh` once |
| W7 Play by invitation         | done        | Unit and loopback tests in `src-tauri/src/worlds/` and `launch/`: a host and a guest through the real tunnel with a stand-in worlds service and world server (whitelist from the renewal, ticket consumed at the login, one place per account, removal kicks and releases, stop cuts the tunnel, a lost session or an exited server ends hosting, strangers get no ticket); DPoP proofs, device registration once per account and a new key when the stored one is taken; player files follow a nick change; the world launch plays the nick and its offline UUID without `--demo`, straight into `127.0.0.1:<port>`, without typed arguments, and is refused without a Microsoft account while that check is on; one room installation per room | Two machines in test mode: with the commands in W8; licensed accounts: Minecraft service approval                                                                               |
| W8 Worlds tab                 | in_progress | Unit tests of `worlds/app.rs` (failure names, device name, idle view) and the permission list in `tests/security_config.rs`; 7 interface tests in `src/features/worlds/`: sign-in first, rooms with their state, opening the own world, joining a friend's, installing then playing, failure and refusal texts, invite codes with a look first, the EULA before a world is made, moderation with a confirmation                                                                                                                                                                                                                                                                                                                                  | Two machines in test mode; keyboard and screen-reader runs                                                                                                                      |
| W9 World safety               | not_started |                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                  |                                                                                                                                                                                 |
| W10 Failures and networks     | not_started |                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                  | Two test players on different home networks, CGNAT access                                                                                                                       |
| W11 Release                   | not_started |                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                  | Windows and macOS machines; signing as for the launcher                                                                                                                         |
| W12 Worlds with mods          | not_started |                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                  |                                                                                                                                                                                 |

## Outside inputs

| Input                                                                                                                     | State                                                       | Needed for                                             |
| ------------------------------------------------------------------------------------------------------------------------- | ----------------------------------------------------------- | ------------------------------------------------------ |
| Minecraft service approval for the project's Microsoft client                                                             | Pending (see [docs/spec/PROGRESS.md](../spec/PROGRESS.md))  | Verified licenses in release builds, live W7 and later |
| worlds.lapi.sh DNS, certificate, bootstrap                                                                                | Done 2026-10-20: worlds.lapi.sh answers                     | W5 deployment                                          |
| Relay VPS, relay.lapi.sh DNS, `RELAY_SSH_HOST`, `RELAY_SSH_KEY`, `RELAY_SSH_KNOWN_HOSTS`, `RELAY_SSH_USER`, `RELAY_TOKEN` | Done 2026-10-20: relay.lapi.sh answers with its certificate | W6                                                     |
| Test players and networks                                                                                                 | Not arranged                                                | W10                                                    |

## W1 report

```text
Phase: W1 — Foundation
Status: done
Delivered: specification 1.0.2 verbatim in spec/, ADAPTATION.md (owner decisions of
  2026-10-19), INTEGRATION.md (adapter map against main at ac201e2), PLAN.md (W1–W12 with exit
  criteria), this file, D-068.
Checks: documentation only; prettier on the new files; no code changed.
Not claimed: any implementation, live provider behaviour, transport or game session.
Next: W2.
```

## W2 report

```text
Phase: W2 — Game and server together
Status: done
Delivered: a hosting permit for one world server beside the main activity permit
  (state/activity.rs). A server runs beside a game and beside a backup or restore of an
  installation; install, repair, relocation and whole-data reset are refused while it runs, and
  it cannot start while they run. The core counts as supervising while a server runs, so
  closing the window keeps it and Quit asks first (lib.rs).
Checks: unit tests (every pair, release on drop, one server at a time, 200-round race against
  an install), the existing activity tests, cargo fmt, clippy -D warnings, the whole test suite.
Not claimed: a real server process (W3), installs of other instances beside a server.
Next: W3.
```

## W3 report

```text
Phase: W3 — Server on the own computer
Status: done
Delivered: downloads.server in descriptors; world tuples (1.21.1); world directories and
  world.json with a backup; typed, escaped server.properties; whitelist.json with offline UUIDs;
  the EULA gate; the server JAR through the verified executor, pinned by SHA-1 and SHA-256; the
  memory budget; a supervised server with readiness, stop, force stop and a run receipt; a
  surviving server holds the hosting permit after a core restart; a status ping and an RCON
  client.
Checks: unit tests (properties injection, nick and UUID, world records, protocol bounds, status
  and RCON against fake servers, output classification that players cannot forge, ports);
  live run of the real server locally (ready in about 20 s, clean stop in about 2 s) and in CI.
Not claimed: Windows and macOS runs of the server; the interface (W8); game rules (W7).
Next: W4 and W5.
```

## W4 report

```text
Phase: W4 — Tunnel
Status: done
Spike: iroh 1.3.0 (MIT or Apache-2.0), QUIC through its own noq fork, TLS with aws-lc-rs as the
  launcher already uses; two endpoints in one process connect directly with no relay and no
  discovery; bidirectional streams carry ordered bytes with finish (half-close), reset and stop.
  The relay side is iroh-relay 1.3.0, pinned with the endpoint for W6.
Delivered: wwf-tunnel/1 admission frames (strict JSON, 16 KiB, gameplay only); the bounded
  incremental Handshake and Login Start parser for protocol 767; the byte bridge (64 KiB per
  direction from a 64 MiB launcher budget, half-close and reset passed on); the host side
  (10 s admission, ticket check through a trait, login name check, then the fixed local server
  connection); the guest listener (5 s classification, local status answers, ticket at
  connection time, bounded pending sockets); QUIC windows sized to the bounds.
Checks: unit and loopback end-to-end tests (A11, A14, A53, A86 as adapted).
Not claimed: two machines, NAT traversal and relay (W6, W7, W10); the real ticket check (W5).
Next: W5.
```

## W5 report

```text
Phase: W5 — worlds.lapi.sh
Status: done (code); deployment waits for the owner's DNS record and one bootstrap run
Delivered: services/worlds (axum, SQLite, port 8788): devices with Ed25519 keys proved by DPoP;
  rooms (name 1–64, description 0–1000, revisions); roles owner, manager, member and states
  active, left, removed, banned; invites (digest stored, 24 h and one use by default, at most
  7 days and 10 uses, last use taken once); host sessions with epochs and 30 s leases renewed
  every 10 s, one live session per room (a new start supersedes); single-use 60 s tickets
  signed with the service's own Ed25519 key (published JWKS); ticket consumption and one slot
  per account; revocations returned at renewal; WebSocket hints; counts and daily backups;
  deploy/worlds, bootstrap, nginx, worlds-service.yml; audience worlds in oauth.
Checks: cargo fmt, clippy -D warnings, 19 tests (A03–A09, A59, A76, A78, A79, A87, A91 as
  adapted); oauth tests with the new audience.
Not claimed: the launcher's client of this API (W7); invite cards in friends chats (W8); the
  maintenance lease for backups (W9); the relay's allow records (W6).
Next: W6.
```

## W6 report

```text
Phase: W6 — Relay
Status: done (code); running waits for the owner's VPS, DNS record and secrets
Delivered: services/relay (lapi-relay): iroh-relay 1.3.0 as a library, pinned with the
  launcher's iroh, with an access control fed by worlds.lapi.sh. The relay asks
  /v1/relay/allow every 10 s with a bearer token; each record lasts at most 30 s; an unlisted
  key is refused, and a connected key whose record ends or drops off the list is closed within
  2 s. At most 8 connections per account (the service sends a hashed group, not the account),
  4 MiB/s per connection with a 4-second burst. Let's Encrypt certificate fetched by the relay
  itself (TLS-ALPN), QUIC address discovery on 7842/udp, metrics on loopback for the counts
  task. worlds.lapi.sh: guests enroll their tunnel key per room (DPoP, renewed every 10 s);
  hosts are listed while their lease holds. deploy/relay (bootstrap, deploy, unit) and
  relay-service.yml with RELAY_SSH_* secrets; a static musl build.
Checks: cargo fmt, clippy -D warnings and tests in services/relay and services/worlds; the
  static release build runs.
Not claimed: a relay on the VPS; two machines on different networks (W7, W10).
Next: W7.
```

## W7 report

```text
Phase: W7 — Play by invitation
Status: done (core); the two-machine run moves to W8, where commands drive it
Delivered: worlds/service.rs, the worlds.lapi.sh client: the account's token on every call,
  DPoP proofs with an Ed25519 device key kept in the credential store, registered once per
  run and account, replaced when the service says it belongs to another account.
  worlds/hosting.rs: a tunnel endpoint in front of a ready world server, the room's session,
  the lease renewed every 10 s; the whitelist follows the members, tickets are consumed at the
  login and the admission's nick is required, revoked admissions are kicked, places are
  released when streams end; hosting ends when stopped, when the server exits or when the
  service ends the session, and the tunnel closes then. worlds/joining.rs: a fresh tunnel key
  enrolled with the relay every 10 s, the host's key from a first ticket, a connection direct
  or through relay.lapi.sh, a local listener for the game with a fresh ticket per login.
  worlds/players.rs: player files move to the new offline UUID after a nick change. Launch:
  a world mode with the launcher nick, its offline UUID, no --demo, --quickPlayMultiplayer to
  the local listener, no typed arguments; refused unless a Microsoft account was signed in
  here, by the same rule as the full game offline (off for now by the owner's decision, until
  Microsoft approves the launcher). One room installation per room, marked as such.
Checks: cargo fmt, clippy and tests; the loopback test runs host, guest and tunnel for real.
Not claimed: commands and interface (W8); two machines and the relay in use (W8, W10).
Next: W8.
```

## W8 report (first part)

```text
Phase: W8 — Worlds tab
Status: in_progress: commands and the tab are in; the friends chat card, presence, the device
  list, the quit choice while hosting and the two-machine run follow
Delivered: worlds/app.rs keeps what this computer does for a room and tells the interface in
  worlds://state: the world's server and Java made ready with download progress, the server
  started, the room's session and tunnel opened; or a hosted room joined as a guest. One room
  at a time; hosting that ends by itself stops the server, and quitting saves the world.
  Nineteen commands: local worlds, Minecraft versions, rooms, a room's page, creating a world
  with its room, the EULA, moderation, leaving, invites, preview and redeem of a code, host,
  stop, join, leave the world, cancel, the room's installation and playing into the room.
  The Worlds tab after Mods: rooms with their state and where the world is kept, create a world
  (the EULA accepted in the form), join with a code after a look at where it leads, a room page
  with members, removing and banning with a confirmation, invite codes shown once, and a status
  panel for getting ready, hosting (players, how it ended, whether it saved) and being a guest
  (connected, why the host refused). Texts in en, ru and ar.
Checks: cargo fmt, clippy and tests; tsc, eslint, prettier, the i18n check and vitest.
Not claimed: the items above that follow; Windows packages (no new window is made).
Next: the rest of W8.
```

## W8 report (second part)

```text
Phase: W8 — Worlds tab
Status: in_progress: the interface is complete; the two-machine run in test mode and the
  keyboard and screen-reader runs follow
Delivered: an invite to a world in the friends chat ("🌍 world <code>"): a room page sends a
  fresh code to a friend in a direct chat, and the card lets the invited player look at where
  it leads, join the room, and opens Worlds on it; anything that is not a well-shaped code
  stays plain text. Presence: while the game plays in a room's world, friends see "In the
  world <name>" (at most 60 characters, as the friends service takes). The device list: the
  account's computers for worlds, this one first and marked; another one is signed out after
  a confirmation, which closes the worlds it hosts. Quit while a world is open asks first
  (keep in the background, close the world and quit, cancel), from the Quit button and from
  the OS or the app menu; the exit saves and closes the world. Texts in en, ru and ar.
Checks: cargo fmt, clippy and tests (device list against the stand-in service, presence text);
  tsc, eslint, prettier, the i18n check and vitest (chat card, presence, sending a code, the
  device list, the quit choice).
Not claimed: two machines; keyboard and screen-reader runs; Windows packages (no new window).
Next: the two-machine run in test mode (worlds.lapi.sh and relay.lapi.sh are live), then W9.
```
