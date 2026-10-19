# Adaptation of the specification to this launcher

This document adapts the Worlds with Friends specification 1.0.2 ([spec/](spec/README.md)) to
this repository. Where the two disagree, **this document wins**; the specification stays as
it was delivered so every change is visible here. The owner asked for the specification to
be treated as a guide rather than a strict contract and to be changed as work goes on
([D-068](../DECISIONS.md#d-068--worlds-with-friends)). Every later change of a default goes
into this file, its tests and the change log at the end.

Phase IDs in this feature are **W1–W12** ([PLAN.md](PLAN.md)). They never mean the base
launcher's P01–P14, and the specification's own P01–P18 are only cited in parentheses.

## A1. The host is the launcher's core, not a separate agent

Specification 02 asks for a separately packaged per-user agent with authenticated local IPC.
This launcher's Rust core already outlives its window: closing the window unloads the
webview while a game or job runs, and only an explicit Quit ends the process (D-017, D-026,
`src-tauri/src/lifecycle/`). The room server and its tunnels are owned by that core.

- No second executable, no named pipe or Unix socket between UI and host. The UI talks to the
  core through the existing typed Tauri commands and events; the webview still never sees a
  token, a device key or a launch command.
- Closing the window keeps hosting. Quit while hosting offers **Save and stop**, **Keep in
  background** (closes the window, the core stays) and **Cancel**. Quitting the core while a
  guest is connected explains that the friend is disconnected.
- The updater never replaces the launcher while a room server runs (drain first, D-063).
- A crashed core with a surviving Java server is recovered like a surviving game
  (`src-tauri/src/launch/orphan.rs`): the run receipt names the process and start identity,
  and the server's RCON listener on 127.0.0.1 with a per-run random password is the trusted
  management channel for a controlled `stop`. A PID alone is never used to kill anything.
- A separately packaged headless agent arrives only with the remote home host (spec P15).

## A2. Identity: license checked once in the launcher, offline-mode servers

Specification 03 requires a trusted game identity hook that verifies the player before
gameplay. The owner chose a simpler model that does not depend on Mojang services being
reachable during play (providers may block them):

- **Who may play in a world.** A player whose Minecraft license was verified by the launcher:
  a Microsoft account signed in to this launcher whose entitlement check
  (`src-tauri/src/auth/decision.rs`) granted play access. The check runs, as today, when the
  account signs in or its profile is created; it is recorded with the account and refreshed
  when the launcher is online anyway, but a room launch never waits for Mojang.
- **Game identity in a world** is the launcher account (D-064): its nick, and the offline UUID
  the server derives from it (`OfflinePlayer:<nick>`, a version 3 UUID). A world launch runs
  the full game under that nick without `--demo`, **only** when a verified Microsoft account is
  present on this computer. Players without a license keep the demo (D-033, D-045) and cannot
  open worlds. Outside worlds nothing changes: no-token profiles and the launcher account
  still play the demo. This narrows the base rule "no offline/cracked/demo bypass"
  (`docs/spec/README.md`) for worlds only, and only for verified owners.
- **Known limit.** The license check lives in the launcher, so a modified build could skip it.
  The owner accepted this risk.
- **Room servers run `online-mode=false`.** No identity mod, Java agent or session server is
  needed on either side; the runtime flavor stays `unmodified`. Who may enter is decided by the
  tunnel (A3), not by Mojang.
- **Skins.** Offline UUIDs are version 3, so the skins mod shows our skins (D-065 rule).
- **Nick changes.** The offline UUID follows the nick. When a member's nick changed, the host
  renames that player's `playerdata`, `stats` and `advancements` files from the old UUID to the
  new one while the server is stopped, before it starts, so the inventory stays.

## A3. Who enters a world: tunnel admission replaces Mojang verification

Specification 07's ticket, lease and release-barrier contracts stay; the pre-play identity
binding becomes a check of the Minecraft login packet by the host's bridge:

1. The server listens only on 127.0.0.1 at a random port. Nothing but the host's bridge
   connects to it; the host's own game also enters through the bridge.
2. A guest gets a single-use ticket (60 s) from worlds.lapi.sh for an active member, this room,
   this session, this device and this host epoch, at the moment the game opens its
   connection, not before launch.
3. The host verifies the ticket's signature and the peer's transport key, consumes the ticket
   at worlds.lapi.sh (device-authenticated), and gets the expected nick.
4. Before forwarding a single byte to the server, the bridge parses the Handshake and Login
   Start packets (version-qualified, bounded at 8 KiB, fragmentation and coalescing handled,
   every consumed byte replayed exactly once) and compares the login name with the expected
   nick. A mismatch, a missing login within 10 s, or a status-to-login switch closes the
   stream; the server never sees it. Status pings are answered locally from the room status
   and never reach the server.
5. Second layer: `white-list=true` and `enforce-whitelist=true` with the members' offline UUIDs,
   kept current by console commands (`whitelist add/remove`, `kick`).
6. One slot per account per room. A second login under the same nick is refused
   (ALREADY_CONNECTED) until the server's log reports that the first one left; a closed TCP
   socket alone does not free the slot. The server's `Name[/127.0.0.1:port] logged in` line
   ties the bridge's local port to the player for kicks and slot accounting.
7. Revocation: removing a member closes their stream within 5 s when the control link is
   healthy and within 30 s otherwise (lease 30 s, renewed every 10 s, conservative deadline
   from the request start, spec 03).

Each room has its own server process, world directory and per-session transport key, so
rooms are isolated from each other.

## A4. The backend: services/worlds on the existing stack

There is no room backend; friends.lapi.sh has only chat rooms. A new service follows the
pattern of friends, skins, mods and feed:

- `services/worlds`, Rust with axum 0.8, tokio and SQLite (rusqlite), at
  `https://worlds.lapi.sh`, loopback port **8788**, deployed under `/home/www/lapi/worlds`
  by `deploy/bootstrap.sh` and a `worlds-service.yml` workflow. No PostgreSQL, no OpenAPI
  generation, no outbox: request and response types are shared Rust structs checked by tests
  on both sides, and events go over one WebSocket like friends `/v1/live`.
- Access tokens are oauth.lapi.sh JWTs checked locally against its JWKS; the account service
  adds the audience `worlds`. Nicks are resolved through the account service's internal API.
- Device-scoped calls (host lease renewal, ticket consumption, relay enrollment) carry DPoP
  proofs (RFC 9449) made with the device's Ed25519 key, which lives in the OS credential vault
  like the refresh token. mTLS is not used: it does not pass Cloudflare.
- Tickets are Ed25519 JWTs signed by worlds.lapi.sh with key IDs and rotation.
- `authorityEpoch` is a random value kept in the service's environment, outside the database,
  and changed after a database restore, so restored rows never revive a lease.
- Logs and workflow output carry no nicks, addresses, room names or world names; the
  `counts` task prints numbers only (repository rule).
- Invitations: a card in the friends chat (friends.lapi.sh gains an invite message kind that
  carries only a room reference and an invite id) and an invite code for people who are not
  friends yet. Defaults from spec 04 stay: 24 h, one use, at most 7 days, member role only.
  An HTTPS landing link and a `launcher://` deep link come later.

## A5. Transport and relay

- iroh inside the core for direct connections with NAT traversal; the version is pinned after
  the W4 spike, not from "latest" documentation.
- **relay.lapi.sh runs on a separate VPS** (owner decision, 2026-10-19): its own
  `RELAY_SSH_*` secrets in GitHub Actions, its own bootstrap, TLS on 443/TCP and UDP served
  directly (Cloudflare does not carry this traffic), so the main server's address stays behind
  Cloudflare. The relay admits only endpoints that worlds.lapi.sh authorized (30 s allow
  records renewed every 10 s), with per-account quotas and counters.
- The public n0 relays may be used only by tests, never by a build players get.
- The relay-only privacy mode moves to W10; core starts with the automatic route.

## A6. World custody without the full maintenance protocol

While a world never leaves its host's computer there is only one writer, so specification 18's
prepare/commit/acknowledge protocol and recovery claims are deferred to host handoff (spec P16).
Core keeps:

- local write-ahead operation receipts in `world.json` (active generation, last mutation)
  with the existing atomic file replacement (`src-tauri/src/storage/atomic.rs`);
- a backend maintenance lease ("world busy", 120 s, renewed every 20 s) that blocks starts
  during import, backup, restore and export, and that never expires into a free start while a
  local operation is unresolved;
- backups of the stopped world through the existing deduplicating store (D-047,
  `src-tauri/src/backup/`), automatically before restore and after a hosting session.

## A7. Other changes

| Specification                                          | Here                                                            | Why                                                            |
| ------------------------------------------------------ | --------------------------------------------------------------- | -------------------------------------------------------------- |
| Release matrix Windows x64, macOS x64/arm64, Linux x64 | Also Windows arm64                                              | The launcher's CI and Package workflows already build it       |
| Metrics dashboards and alert thresholds for the relay  | Numeric counters in a `counts` task and a check in `health.yml` | Same as every other service; workflow logs are public          |
| Local IPC request caps and peer authentication         | Tauri command argument limits and validation                    | There is no separate IPC channel (A1)                          |
| `crates/wwf-*` workspace crates                        | Modules `worlds/`, `server/`, `tunnel/` in `src-tauri/src/`     | The launcher is one crate; split only if build times demand it |

What stays as specified: leases of 30 s renewed every 10 s with deadlines from the request
start; 60 s single-use tickets; tunnel buffers of 256 KiB per stream direction and 64 MiB per
launcher; at most 4 unadmitted streams per endpoint context and 64 per host; no destination,
port, path or command ever accepted from a guest; startup deadline 180 s and stop grace 60 s;
backups only of a stopped world; server output drained and filtered so chat and nicks are not
stored by default.

## A8. First support tuple

Minecraft 1.21.1 without a loader, the official server JAR from Mojang's version manifest
(`downloads.server`, SHA-1 and size checked, SHA-256 recorded), Java 21 from the existing
runtime provider, runtime flavor `unmodified`, `online-mode=false`. 26.x follows as the second
tuple; Fabric and other loaders arrive in W12.

## Change log

- 2026-10-19 — First adaptation (D-068): host in the core, license once in the launcher and
  offline-mode servers, tunnel admission with a Login Start check, services/worlds on SQLite,
  DPoP device keys, relay on a separate VPS, deferred maintenance protocol, 1.21.1 first.
- 2026-10-19 — W3: the server JAR is stored once per version in `shared/servers/<sha1>/` and
  pinned by each world, instead of per-world server generations, until room packs need them
  (W12). The server's console output is classified and dropped; no server log is kept.
- 2026-10-19 — W4: iroh pinned at 1.3.0 with aws-lc-rs and the port mapper, no address lookup
  service (endpoint addresses come from the worlds API) and a fresh key per room session. The
  bridge uses 64 KiB per direction, within the 256 KiB cap. The guest also checks the login name
  before asking for a ticket, so a wrong name never spends one.
