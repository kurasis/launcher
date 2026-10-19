# Plan

Twelve phases, W1–W12. Each phase lands as one or more pull requests merged when CI is green;
state is kept in [PROGRESS.md](PROGRESS.md). The specification phases each one covers are in
parentheses, and the acceptance cases it is measured against are those of
[spec/ACCEPTANCE.md](spec/ACCEPTANCE.md) as changed by [ADAPTATION.md](ADAPTATION.md).

The first play with friends becomes possible after W7; the full release follows W11. Live
checks with real licensed accounts wait for Minecraft service approval (see PROGRESS);
everything else is built and tested before it, using a debug-only test mode that is marked
in the window and absent from release builds.

| Phase                               | Depends on | Outcome                                                                               |
| ----------------------------------- | ---------- | ------------------------------------------------------------------------------------- |
| W1 Foundation (P01)                 | —          | Specification, adaptation, integration map, plan and decision D-068 in the repository |
| W2 Game and server together (P03)   | W1         | A hosting permit for one world server beside the main activity permit                 |
| W3 Server on the own computer (P06) | W2         | A real 1.21.1 server installed, started and stopped cleanly                           |
| W4 Tunnel (P04)                     | W1         | iroh between two launchers, admission frame, bounded TCP bridge, Login Start check    |
| W5 worlds.lapi.sh (P02, P07)        | W1         | Rooms, roles, invites, devices, host leases, tickets, events                          |
| W6 Relay (P05)                      | W4, W5     | relay.lapi.sh on its own VPS with access control and quotas                           |
| W7 Play by invitation (P07, P08)    | W3, W5, W6 | Two players in one world through the tunnel                                           |
| W8 Worlds tab (P10)                 | W7         | The complete interface in en, ru and ar                                               |
| W9 World safety (P09)               | W3, W7     | Import, backup, restore and export without data loss                                  |
| W10 Failures and networks (P11)     | W8, W9     | Recovery and real network results recorded                                            |
| W11 Release (P12)                   | W10        | Worlds in the stable channel on all five package targets                              |
| W12 Worlds with mods (P13, P14)     | W11        | Fabric first, then NeoForge, Forge and Quilt, from mods.lapi.sh                       |

Later, each by its own owner decision: a home host without the launcher and with a separate
agent (P15), moving a world to another host (P16), cloud hosting (P17), voice and opening a
single-player world from the game (P18).

## W1 — Foundation

- `docs/worlds-with-friends/`: the delivered specification in `spec/`, ADAPTATION, INTEGRATION,
  PLAN, PROGRESS; D-068 in `docs/DECISIONS.md`.
- Exit: documents merged; base checks green on main; blockers named with owners.

## W2 — Game and server together

- `state/activity.rs` keeps its main permit and gains a hosting permit for one world server. A
  server runs beside a game and beside a backup or restore of an installation; install, repair,
  data relocation and whole-data work (reset) are refused while it runs, and a server cannot
  start while they run, because they may replace shared artifacts the server's Java reads.
- The core counts as supervising while a server runs: closing the window keeps it, and an
  application Quit asks first.
- Installs of other instances beside a running server (with pins on live files) are a later
  refinement, not part of core.
- Exit: tests for every pair of activities and for racing a server against an install; existing
  activity tests still pass.

## W3 — Server on the own computer

- Parse `downloads.server` in descriptors; plan, download and verify the server JAR through the
  download executor into `shared/servers/<sha1>/server.jar`, shared by every world of the
  version; the world pins it by SHA-1, SHA-256 and size (`worlds/install.rs`).
- World directories `worlds/<worldId>/` with `world.json` (schema version, version, settings,
  server pin, EULA acceptance, the operation changing the world, how the last run ended) and a
  `server/` working directory. Typed `server.properties` written before every start (seed,
  preset, difficulty, game mode, hardcore, PvP, player limit; `online-mode=false`,
  `white-list=true`, `enforce-whitelist=true`, loopback bind, random ports, RCON with a per-run
  password); `whitelist.json` with offline UUIDs; `eula.txt` only after the player accepts the
  EULA. Game rules are set through console commands in W7.
- Memory budget before start: game heap, server heap and the reserve the game's heap check
  uses; an impossible budget is refused.
- Server run receipt `locks/world-server.json`: a server that survives a core restart keeps the
  hosting permit until it exits and is stopped through RCON, never by PID.
- Supervision through `lifecycle/child.rs` with a piped stdin: readiness from the `Done (` line
  plus a local status ping, `stop` through stdin with 60 s grace, explicit force stop recorded as
  unclean, output classified and dropped (no server log kept).
- Testing without a room: `src-tauri/tests/live_server.rs`; commands for the interface arrive with
  W8.
- Exit: CI on Linux installs and runs the real 1.21.1 server (A15, A17, A19, A20); occupied port
  and junk input tests.

## W4 — Tunnel

- Spike: iroh endpoint in the core, two endpoints in one test process and across two machines;
  record the version, licence, stream semantics, relay protocol and memory, then pin it.
- `wwf-tunnel/1`: 4-byte length + JSON admission frame up to 16 KiB, 10 s deadline, service
  `gameplay` only; bidirectional TCP bridge with backpressure, half-close and reset propagation;
  256 KiB per stream direction, 64 MiB per launcher; unadmitted stream limits.
- Guest listener on 127.0.0.1: local status answers, Login Start parsing, the consumed prefix
  replayed exactly once.
- Exit: A11, A14, A86 and the A53 variant (status label cannot carry gameplay) pass in tests.

## W5 — worlds.lapi.sh

- `services/worlds` (axum, SQLite), port 8788, `deploy/bootstrap.sh`, nginx, `worlds-service.yml`
  with test and deploy jobs and a `counts` task; audience `worlds` in oauth.
- Accounts by oauth subject; devices with Ed25519 keys and DPoP; rooms (name 1–64, description
  0–1000, revisions), membership states and roles, invites (digest stored, 24 h, one use,
  at most 7 days), start and stop operations with cancellation, host leases with epochs,
  tickets, ticket consumption and admission leases, maintenance lease, WebSocket events.
- Exit: tests for the permission table, last-use invite races, two hosts racing, ticket replay,
  delayed renewals, removed member (A03–A09, A59, A76, A78, A79, A87, A91 as adapted).
  Owner sets up DNS and runs bootstrap once.

## W6 — Relay

- `deploy/relay/`: pinned iroh-relay build, TLS, UDP, systemd; access policy fed by worlds.lapi.sh
  (30 s allow records renewed every 10 s); per-account connection and byte limits; counters.
- Workflow with `RELAY_SSH_*` secrets; the main server's address never appears there.
- Exit: unregistered endpoint refused, revoked busy endpoint closed, renewal stopped closes the
  connection (A12, A58). Owner provides the VPS and DNS.

## W7 — Play by invitation

- Host: start a room, renew the lease, keep the whitelist, consume tickets, check the nick, slot
  accounting, revocation, migrate player files after a nick change.
- Guest: room instance, verified license, launch under the launcher nick without `--demo`,
  ticket at connection time, direct or relayed tunnel.
- Exit: two launchers on two machines play in one world in test mode; wrong nick, removed
  member, replayed ticket and second login refused (A10 as adapted, A21, A22, A54, A55, A80).
  The same with real licensed accounts once Minecraft service access is approved.

## W8 — Worlds tab

- Worlds list with host states and last observation time, create world, room page with members,
  roles and invites, invite card in the friends chat, presence, join preparation with each stage,
  hosting panel (players, health, route, save and stop), quit dialog, device list.
- Exit: keyboard and screen-reader runs in en, ru and ar; reopening the window reattaches to a
  running server (A18, A29, A36, A42); Windows package check for any new window.

## W9 — World safety

- Import a single-player world as a copy (the source installation stopped), backup of the stopped
  world through D-047, restore with a preview and a backup first, export as zip.
- Exit: interruption at every write boundary, low disk, corrupt files (A24–A27, A60–A63, A82, A83
  as adapted).

## W10 — Failures and networks

- Core crash with a living server, sleep and resume, relay and API outages, delayed renewals;
  relay-only privacy mode.
- Real networks: same LAN, two home NATs, CGNAT, no UDP, mixed operating systems.
- Exit: A13, A28, A33–A35, A56, A57, A75, A89 recorded with real observations.

## W11 — Release

- Packages for linux-x64, windows-x64, windows-arm64, macos-arm64 and macos-x64; Windows package
  check; secrets in logs and exports; 2-hour five-player session; update postponed while hosting.
- Exit: A31, A32, A37–A41, A44, A72–A74, A84, A85, A90 as adapted.

## W12 — Worlds with mods

- Room pack generations built from mods.lapi.sh (client and server sides), Fabric first, then
  each further loader only after its own test matrix passes.
- Exit: A45–A47.
