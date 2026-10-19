# Progress

States: `not_started`, `in_progress`, `blocked`, `done`. A phase whose code is finished but
whose live check waits for an outside input is `blocked`, with the input named. Evidence says
whether it is a unit test, a local run, CI or a live run with real accounts and networks.

| Phase                         | Status      | Evidence                                                                                                                                                                                                                                                                                         | Blocker / notes                                                                  |
| ----------------------------- | ----------- | ------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------ | -------------------------------------------------------------------------------- |
| W1 Foundation                 | done        | This directory and [D-068](../DECISIONS.md#d-068--worlds-with-friends); documents only, no code                                                                                                                                                                                                  | —                                                                                |
| W2 Game and server together   | done        | Unit tests in `src-tauri/src/state/activity.rs` for every pair of activities and a 200-round race of a server against an install                                                                                                                                                                 | The permit has no caller until W3 starts a server                                |
| W3 Server on the own computer | done        | Unit tests in `src-tauri/src/worlds/`; `tests/live_server.rs` run locally and in the CI job `World server (linux-x64)`: the real 1.21.1 server prepared, ready, RCON checked, stopped clean, then force stopped and recorded unclean                                                             | Windows and macOS runs of the server come with W10 and W11                       |
| W4 Tunnel                     | done        | iroh 1.3.0 pinned; 16 tests in `src-tauri/src/tunnel/` over loopback endpoints: exact bytes with fragmentation, both directions at once, half-close, reset, wrong ticket, wrong name, local status, status declared as gameplay, bounded unadmitted streams, backpressure, bounded local sockets | Two machines on different networks: W7 and W10                                   |
| W5 worlds.lapi.sh             | not_started |                                                                                                                                                                                                                                                                                                  | Owner: DNS record and certificate for worlds.lapi.sh, `deploy/bootstrap.sh` once |
| W6 Relay                      | not_started |                                                                                                                                                                                                                                                                                                  | Owner: a VPS, DNS record relay.lapi.sh (not proxied), `RELAY_SSH_*` secrets      |
| W7 Play by invitation         | not_started |                                                                                                                                                                                                                                                                                                  | Live check with licensed accounts needs Minecraft service approval               |
| W8 Worlds tab                 | not_started |                                                                                                                                                                                                                                                                                                  |                                                                                  |
| W9 World safety               | not_started |                                                                                                                                                                                                                                                                                                  |                                                                                  |
| W10 Failures and networks     | not_started |                                                                                                                                                                                                                                                                                                  | Two test players on different home networks, CGNAT access                        |
| W11 Release                   | not_started |                                                                                                                                                                                                                                                                                                  | Windows and macOS machines; signing as for the launcher                          |
| W12 Worlds with mods          | not_started |                                                                                                                                                                                                                                                                                                  |                                                                                  |

## Outside inputs

| Input                                                                                                      | State                                                      | Needed for                                             |
| ---------------------------------------------------------------------------------------------------------- | ---------------------------------------------------------- | ------------------------------------------------------ |
| Minecraft service approval for the project's Microsoft client                                              | Pending (see [docs/spec/PROGRESS.md](../spec/PROGRESS.md)) | Verified licenses in release builds, live W7 and later |
| worlds.lapi.sh DNS, certificate, bootstrap                                                                 | Not started                                                | W5 deployment                                          |
| Relay VPS, relay.lapi.sh DNS, `RELAY_SSH_HOST`, `RELAY_SSH_KEY`, `RELAY_SSH_KNOWN_HOSTS`, `RELAY_SSH_USER` | Owner chose a separate VPS (2026-10-19); not provided yet  | W6                                                     |
| Test players and networks                                                                                  | Not arranged                                               | W10                                                    |

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
