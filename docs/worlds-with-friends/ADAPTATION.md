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
  adds the audience `worlds`. Nicks come from the tokens (change log, W5).
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
- The relay-only privacy mode (W10) is one setting of the player, for hosting and joining alike,
  instead of a choice per room: every session already has a fresh key, so the two modes never
  share an endpoint.

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
port, path or command ever accepted from a guest; stop grace 60 s;
backups only of a stopped world; server output drained and filtered so chat and nicks are not
stored by default.

## A8. World versions

Every Minecraft release from 1.7.6 on, without a loader (owner's decision, 2026-10-20). 1.7.6
is the first server that keeps players by UUID (`whitelist.json`, `playerdata/<uuid>.dat`),
which the whitelist of offline UUIDs needs. With "Show snapshots" on when a world is made
(off by default, owner's decision 2026-10-20), snapshots from 14w17a (the first after 1.7.6)
and the pre-releases and release candidates of those releases are offered too, with a note that
a snapshot can damage a world and that friends need the same snapshot. With "Show special
versions" on (D-073), the versions of the launcher's own list with a server from 1.7.6's day on
are offered after them by name (April Fools versions from 15w14a, Combat Tests, experimental
snapshots, earlier builds), with their server JARs from upd.lapi.sh only; their servers read the
properties as a snapshot's.
The official server JAR comes from the version's descriptor (`downloads.server`, SHA-1 and size
checked, SHA-256 recorded), from mirror.lapi.sh when Mojang fails, as every Mojang file; the
mirror keeps every release's server JAR ahead of time; Java is the one the descriptor names, from the existing
runtime provider; runtime flavor `unmodified`, `online-mode=false`. What changed between
releases is handled by release, not by a table of versions:

- the protocol is learned from the server's own status ping when it is ready; the host's
  tunnel admits only logins of that protocol, and the guest takes any release's;
- Login Start is read in the layout of the client's protocol (name only up to 1.18.2, the
  optional signing key of 1.19, the optional UUID of 1.19.1 to 1.20.1, the UUID from 1.20.2);
- difficulty and game mode are written as numbers before 1.14, `level-type` as the world
  type's name before 1.19; for a snapshot, whose id does not say which it reads, always the old
  forms, which every server from 1.7.6 to 26.3 still reads (26.3's `DedicatedServerProperties`
  reads numbers and maps the old type names);
- a snapshot's protocol (bit 30 set) says nothing about its Login Start layout, so the tunnel
  takes whichever known layout fits; the host still admits only its server's protocol;
- Log4Shell: the logging configuration drops any message with `${...}` before it is formatted
  (Mojang's fix for 1.7 to 1.11), adds `{nolookups}` (1.12 to 1.16) and
  `log4j2.formatMsgNoLookups`, and is passed as `log4j.configurationFile`, which every Log4j 2
  reads;
- `Done (...)! For help, type "help" or "?"` (before 1.13) also means ready.

CI runs the real server of 1.7.10, 1.12.2, 1.16.5, 1.18.2, 1.19.2, 1.20.1, 1.21.1 and the
newest release and snapshot, and the Fabric server of 1.20.1, 1.21.1 and 26.3 with mods (W12).
Other loaders follow in W12.

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
- 2026-10-19 — W5: a member's nick is the one their latest access token carries (the account
  service puts it in every token), instead of a call to its internal API; access tokens live 10
  minutes. A room is bound to the owner's world ID, not to a device: the owner hosts from any
  of their registered devices, and a new start supersedes the live session, so a crashed host
  recovers at once. A ticket is used up even when its admission is then refused. The
  maintenance lease moves to W9 with backups, and start and stop are single calls instead of
  operations with cancellation.
- 2026-10-19 — W6: the relay is iroh-relay 1.3.0 built into our own binary, not the stock
  server: the stock one checks a key only when it connects. The relay pulls the allow list from
  worlds.lapi.sh every 10 s (worlds.lapi.sh does not reach the relay's server); guests enroll
  their per-session key for a room, and hosts are allowed while their lease holds. The relay
  sees a hashed group per account, never the account. Its certificate comes from Let's Encrypt
  through the relay itself, so the VPS needs no nginx or certbot.
- 2026-10-19 — W7: the guest's licence is the one the launcher already checked: a Microsoft
  account signed in on this computer (its entitlement was checked at sign-in), with no check
  per launch: the same rule as the full game offline, which the owner has turned off until
  Microsoft approves the launcher. A world launch adds `--quickPlayMultiplayer` itself, as
  friends' server invites do, instead of turning on the descriptor's quick-play feature,
  whose placeholders the plans do not know. The host keeps
  each member account's last nick per world and moves player files to the new offline UUID
  before a renamed member logs in. The commands and the two-machine run move to W8, with the
  interface that uses them.
- 2026-10-19 — W8: one computer is in one room at a time, hosting or as a guest, as one hosting
  permit allows one server. The host's own game connects to its world server on loopback, not
  through the tunnel. An invite is a code shown once, pasted or sent in a chat; there is no
  invite web page. The device list names a computer by its system and architecture, never its
  host name. Worlds is not offered as a start screen: rooms need the service.
- 2026-10-20 — W9: no maintenance lease on worlds.lapi.sh. A world lives on one computer, so
  backups, restores, imports and exports take a local claim instead: one at a time, never while
  that world's server runs or a room gets ready, and hosting waits for them. Backups of worlds
  use the D-047 store in a store of their own under `worlds/backups`; the last 10 automatic
  ones (after a session that changed the world) are kept, ones made by hand stay. An export
  holds the save only, never `server.properties` (console password) or the whitelist (names).
  Import is for installations without mods until W12.
- 2026-10-20 — after the first two-machine run: an install of an installation may run while a
  world server runs, as the host installs the room's game only after opening its world (Play
  refused it). An install adds missing files and a runtime goes into its own
  content-addressed folder, so the server's Java is not replaced. Repair, relocation and
  reset still wait for the server.
- 2026-10-20 — W10: the host tells the service how many players its server has in every
  renewal, and a room's online count is the larger of that and its open admissions, so the
  host's own game (on loopback, never admitted) is counted. A lapsed lease is never renewed: the
  host opens a new session on a fresh endpoint while its server runs, and guests reconnect to it
  by themselves. A world server that outlived its launcher is stopped through RCON at the next
  start instead of being adopted.
- 2026-10-20 — A8 (owner): every release from 1.7.6 on instead of 1.21.1 alone, and the mirror
  keeps every release's server JAR (still the fallback when Mojang fails). The tuple table is gone: the protocol comes from the server, the
  Login Start layout from the client's handshake, Java from the descriptor, and the few
  properties and the logging configuration that changed are written per release.
- 2026-10-20 — A8 (owner): snapshots behind a switch in the new-world form, off by default.
- 2026-10-21 — W12 (owner, plan doc): a world with mods is made from an installation with a
  loader, or imported from one. Its mods are an immutable generation in the world
  (`worlds/<id>/pack/`, files by SHA-256, a `.mrpack` with `env` per file), only catalog files,
  the side of each from its jar, then the catalog, else both. The server is the loader's server
  profile with pinned libraries, never the loader's installer; its `mods/` is rebuilt before each
  start and settings are written only where missing. Fabric first, offered for the releases its
  live test covers (1.20.1, 1.21.1, 26.3); releases only, no snapshots.
- 2026-10-21 — W12 guests (owner, plan doc): the room pack is the generation's client side,
  published to the catalog's own packs with `room` (status `room`: never searched, shown or
  listed) when the world is first hosted with that generation, as a version of the world's room
  pack. The session at worlds.lapi.sh keeps only its catalog version and SHA-256 and names them
  in the room and in tickets. A guest's room installation gets it as any catalog pack, after a
  consent that names mods new to that computer; a yes holds for that pack, an update with new
  mods asks again. Play refuses while the installation holds another pack than the room plays.
- 2026-10-21 — W12 updates (owner, plan doc): an update builds the next generation from an
  installation with the same loader and Minecraft version. A server setting the host changed
  that the new generation brings another version of is a conflict (keep mine / take the new
  one, shown in three versions); one the host never changed takes the new version. The stopped
  world is backed up, the generation is tried on a copy of that backup (a world of its own,
  `trialOf`, never listed), and only then kept, with the earlier generation, the backup and the
  replaced settings recorded as the way back. A hosted world closes for the update and opens
  again. The first start confirms it; a start that fails puts all of it back. Every backup
  records its generation, and a restore brings that generation back; generations no backup
  needs go with their files.
- 2026-10-21 — W12 Quilt (owner, plan doc): Quilt works as Fabric does, through the server
  profile of meta.quiltmc.org; Quilt Loader finds Mojang's server by `loader.gameJarPath`, its
  name for Fabric's `fabric.gameJarPath`. Its live test runs Fabric mods from the catalog, which
  Quilt loads, since the catalog has no Quilt-only mod. Offered for 1.20.1, 1.21.1 and 26.3.
- 2026-10-21 — W12 NeoForge (owner, plan doc): no installer runs. The launcher reads the
  installer's install profile for the server side (its `server` data, its server processors,
  without the step that only unpacks start scripts, with `{ROOT}/libraries/` in the Maven tree)
  and runs those processors with Mojang's verified server JAR and server mappings. Each file
  they build is recorded by its SHA-1 in `metadata/loaders/<sha1>.server-files.json` and checked
  before every start. The server starts with the installer's `data/unix_args.txt` or
  `data/win_args.txt`; a token that would load an agent, change the boot class path, set a
  `-XX:` option or read an argument file refuses the installer. FML replaces the server's
  logging configuration with its own, so A7 is kept by `forge.logging.file.level=off` and
  `forge.logging.debugFile.level=off`: its log files stay empty; its console lines carry the
  logger name, which the console reader accepts. Offered for 1.21.1 and 26.3.
- 2026-10-21 — W12 Forge (owner, plan doc): Forge goes the NeoForge way: its installer read
  for the server side, its server processors run by the launcher. Installers without
  start-arguments files get a command the launcher builds: Forge 1.12.2 and 1.16.5 from the
  manifest of the loader JAR the install profile names (`Main-Class`, and a `Class-Path` that
  may only name files under `libraries/` and `minecraft_server.<version>.jar`); Forge 26.x from
  its shim's own list and properties, whose bootstrap the launcher starts directly, without the
  shim's tuning options. Every file a command names is checked before each start. Offered for
  1.12.2, 1.16.5, 1.20.1 and 26.3; the catalog has no mod for Forge 26.3 yet, so its test starts
  Forge alone. OptiFine has no server.
- 2026-10-21 — W12 coverage (owner asked for the widest coverage of mods): worlds with mods
  for every release where the loader has a stable build. Fabric from 1.14, Quilt from 1.14.4,
  NeoForge from 1.20.1 and Forge for 1.7.10, 1.8.9, 1.12.2 and its recommended releases from
  1.14.4 (Forge calls every other build a beta), all up to 26.3. The list is
  `src-tauri/src/worlds/tested.json`: every change runs its `on_every_change` releases (the oldest
  and newest of each loader and one of each kind of installer), the nightly run and a manual run
  all of `tested`, and a release joins `tested` only after it passed. For each release the live
  test takes the first catalog mod with a release for it (Fabric API on Fabric and Quilt; JEI,
  AppleSkin, Clumps, FerriteCore, Cloth Config, spark or Bookshelf on Forge and NeoForge) and proves it loaded:
  Fabric API's data in `datapack list`, the mod's id in the save's `level.dat` (FML and NeoForge
  record it). Forge 1.7.10 and 1.8.9 servers come from the old installer format: Forge and the
  libraries it marks for the server, its universal jar, started from that jar's manifest.
- 2026-10-22 — Starting a server (owner): the startup deadline of 180 s counts from the server's
  last line instead of from spawn, so a server that keeps writing (a big pack on a slow computer)
  gets time, up to 20 minutes in all; a server silent for 180 s is stopped as before. A start
  that fails says why where the loader or the start's crash report names a mod: the mod, what it
  needs (a missing mod, another Minecraft version, a newer loader build or Java) and what the
  world has, or the mod that failed (`src-tauri/src/worlds/failure.rs`). While the server
  starts, before anyone can join, only those values are kept from its lines; a crash report is
  read from the world's own `crash-reports/`, written during this start, at most 4 MiB, and
  never leaves the computer. CI starts six worlds whose mods do not start (NeoForge, Fabric,
  Forge before and after 1.13) and checks the mod is named.
- 2026-10-22 — Making a world with mods (owner): before a world is made or its mods are updated,
  the jars of its server mods are checked against the world's version and loader build with the
  shared `mod_meta` reader (D-066). A mod that needs a mod the world lacks, another Minecraft
  version, a newer loader build or another mod version, or that does not start with a mod there,
  stops the world with the same words as a failed start, naming the mod by its catalog title.
  Only what the jars say for certain counts; the rest is left to the start. Choosing a release
  that runs on the installation's loader build is the Mods tab's own (D-066), so a world made
  from such an installation passes. Of the six CI worlds whose mods do not start, four are now
  refused before they are made, and two (a mod that crashes, a pre-1.13 version range) by the
  start.
- 2026-10-23 — Forge before 1.13: before every start the launcher makes the world's
  `config/forge.cfg` turn Forge's update check off (`general { B:disableVersionCheck=true }`).
  Under Java 8 that check's thread can deadlock with the server loading Netty's epoll library,
  and the start hangs after "Starting Minecraft server on" (seen in CI on Forge 1.12.2). Forge
  writes its other settings into the file itself; a file from the world's mods keeps what it
  says, with the setting added after it (`src-tauri/src/worlds/forge_cfg.rs`).
