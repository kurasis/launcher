# 08 — Implementation order and acceptance criteria

## Milestones

| Gate | Deliverable | Exit criterion |
| --- | --- | --- |
| G0 — Feasibility | Own client registration, current auth contract, source probes | At least one real positive Java access/profile result for this project; no borrowed ID; auth decision fixtures pass; source URLs/hashes and runtime availability checked. Full real account-state matrix is G5. |
| G1 — Desktop shell | Tauri/React scaffold, Rust state, snapshot IPC, window lifecycle | Window destroy/recreate and child supervision work on target platforms using a harmless test process |
| G2 — Installer | Metadata parser, rules, planner, downloader, runtime, storage | Baseline artifacts install with verified hashes; cancellation/repair tests pass |
| G3 — Real launch | Auth, complete launch plan, Java process and logs | One baseline reaches the game menu and a local world; returns to launcher on exit |
| G4 — Product UI | Real account/installation/settings/download/error screens | No mocked production paths; full first-use journey works |
| G5 — Qualification | Second baseline, security tests, platform smoke matrix, packaging | Required acceptance items pass; remaining external gates explicitly reported |

G0 can be investigated while building the shell. An externally blocked G0 does not prevent pure resolver/UI work, but G3/G5 cannot be declared passed. Do not spend most of the project polishing a UI before verifying a real login and launch path.

## Execution order

Use [PHASES.md](PHASES.md) and its P01–P14 task files as the authoritative implementation sequence. G0–G5 above remain quality checkpoints; A01–A26 below remain the final acceptance criteria. The phase files assign concrete tasks, deliverables, tests and dependency handling. Track actual progress in [PROGRESS.md](PROGRESS.md).

## Automated test requirements

Prefer deterministic tests around real invariants over dozens of snapshots of implementation details. Network tests use an isolated local fixture server with synthetic data. Only the test binary may permit insecure localhost artifact sources; production builds must not ship this override.

| Test group | Required cases |
| --- | --- |
| Catalog/descriptor | Exact version lookup, hash mismatch, missing mandatory field, unexpected ID, unsupported metadata, stale cache |
| Rules | No rules, ordered allow/disallow, last match, OS mismatch, architecture, OS regex, absent=false features, conditional string/array |
| Plan | Deduplicated assets, deterministic ordered classpath, no foreign-OS entries, modern native JAR handling, per-platform Java major |
| Download | Success, early EOF, wrong size/hash, timeout, retry/backoff, rate-limit delay, cancellation, partial restart, concurrent duplicate file |
| Extraction | Traversal, absolute path, drive path, symlink escape, safe internal link, oversized expansion, duplicate/case-colliding entry |
| Storage | Interrupted platform-specific replacement, upstream bytes unchanged, draft hash nullability, unknown app schema, corrupt settings recovery, global activity exclusion, stale PID and ambiguous spawn recovery |
| Auth | PKCE/state lifecycle, replay, cancel/timeout, refresh rotation, no-access, valid subscription, profile missing, `401`, `403`, `429`, malformed response |
| Access enforcement | UI calls launch despite disabled button; Rust still denies without fresh verified access; fixtures cannot authorize production |
| Launch | Spaces/Unicode/quotes/backslashes, Java 21/25 argfile locale/encoding, long Windows classpath, filtered JVM environment, exact metadata flags, missing placeholder, memory bounds, spawn failure |
| Process/logging | Simultaneous large stdout/stderr, bounded buffer, chunk-split secrets, early exit, crash, stop confirmation, no unrelated process kill |
| Window lifecycle | Game exits during destruction, reopen while running, duplicate launch request, exactly one restored window, idle quit versus keepalive |
| IPC/UI | DTO compatibility, epoch/revision race, event gap/reconnect, repeated request IDs, invalid input, duplicate subscription, keyboard-only primary flow |
| Cross-instance/runtime | Same asset-index ID with different hashes, no repair during game, extracted Java corruption detected before execution, pinned runtime repair with latest unavailable |
| Auth concurrency | Late callback after cancel/sign-out, simultaneous refresh, vault deletion failure across restart, unknown 403 classification, browser-close timeout |

## End-to-end acceptance checklist

- [ ] A01 — Clean supported machine installs the launcher without developer tools.
- [ ] A02 — Windows missing WebView2 is handled by the installer; macOS uses system WebView; Linux package dependencies are verified.
- [ ] A03 — Own project client ID completes real browser login and obtains current Java access/profile.
- [ ] A04 — Account with no game access cannot launch; the reason is accurate.
- [ ] A05 — Active Game Pass access works through the chosen current entitlement contract; permanent ownership is not incorrectly required.
- [ ] A06 — Profile-not-created and service/network failure appear as distinct outcomes; neither is treated as a license success.
- [ ] A07 — Tokens do not appear in frontend storage, command previews, regular logs or exported diagnostics.
- [ ] A08 — Vault-backed persistence survives restart; vault-unavailable mode is explicitly session-only with no plaintext fallback.
- [ ] A09 — Version catalog is fetched from Mojang and filtered by the compatibility list; existing instances never silently update.
- [ ] A10 — First install downloads a compatible private Java and all game artifacts with digest verification.
- [ ] A11 — Java selection follows the descriptor: 1.21.1 uses Java 21 and 26.3 uses Java 25 in the qualified baseline.
- [ ] A12 — Play reaches the Minecraft menu and can create/load/save a local test world on each required target.
- [ ] A13 — A licensed account can join an authorized online-mode test server, proving usable game-session credentials; do not test against an unrelated server without permission.
- [ ] A14 — Cancelling/restarting an installation reuses completed valid files and does not present partial data as ready.
- [ ] A15 — Repair fixes a corrupted game artifact and a corrupted extracted runtime, uses pinned inputs, blocks during any active game, and leaves test worlds/options byte-for-byte unchanged.
- [ ] A16 — Long paths, spaces and non-ASCII user directories work, or an explicit supported path constraint is documented before install.
- [ ] A17 — Closing the GUI while playing does not kill Minecraft; the supervisor remains and no second game starts.
- [ ] A18 — Default gameplay mode destroys the webview and recreates it after normal exit and after crash.
- [ ] A19 — Reopening the launcher while the game runs restores the existing supervisor UI; tray absence does not strand the user.
- [ ] A20 — Logs stay bounded and Java does not freeze from unconsumed output pipes.
- [ ] A21 — All required frontend controls invoke real backend behavior; no placeholder success or mocked production auth.
- [ ] A22 — Network outage, expired token, disk full, permission failure and checksum mismatch each provide a meaningful recovery path.
- [ ] A23 — Twenty lifecycle stress cycles (test child permitted) show no duplicate windows/processes, accumulating listeners or sustained resource growth; additionally test real game normal exit and crash on every required target.
- [ ] A24 — Installer upgrade preserves settings, credentials and game data; uninstall preserves user data by default.
- [ ] A25 — Build/test commands and target artifacts are reproducible from the checked-in lockfiles and documentation.
- [ ] A26 — Release report identifies signed/unsigned status, tested targets, descriptor hashes, game/runtime versions, candidate/final package hashes, final-package smoke results and any external blocker.

No real credentials are required for ordinary CI. A03–A06 and A12–A13 need owner-operated real account testing. If no Game Pass test account is available, report A05 as unverified and do not claim full subscription qualification based solely on a fixture. Never purchase subscriptions, create accounts or publish releases as an implicit part of implementation.

## Required real-machine matrix

| Target | GUI/install | 1.21.1 + Java 21 | 26.3 + Java 25 | Return after exit/crash | Package |
| --- | --- | --- | --- | --- | --- |
| Windows x64 | Pending | Pending | Pending | Pending | NSIS |
| macOS Intel | Pending | Pending | Pending | Pending | DMG |
| macOS Apple Silicon | Pending | Pending | Pending | Pending | DMG |
| Linux x64 X11 | Pending | Pending | Pending | Pending | DEB + AppImage |
| Linux x64 Wayland | Pending | Pending | Pending | Pending | At least one qualified format |

Fill actual OS versions, hardware/GPU, launcher commit, runtime build, test date and observed result. These initial Pending values are intentional: no application exists yet. If a newer Minecraft release needs different hardware/graphics support, report that independently from launcher failure and keep compatibility claims narrow.

## Definition of done

The MVP is done when real source code implements FR01–FR16, required automated tests pass, required real-machine/game/auth checks are complete, packaging is usable, and the owner can follow the developer and user documentation without unstated steps. UI screenshots, compilation alone, or simulated progress are insufficient. A blocked authentication approval or an untested operating system must be visible in the final report, not buried in a TODO.

## Post-MVP backlog

Only after the above gates: Fabric first, then other loaders; modpack import with archive/path safety; richer account management; explicit offline policy; automatic signed launcher updates; instance deletion/backups; more releases; runtime maintenance UI; localization; per-instance custom Java/advanced JVM settings with validation. Each adds its own compatibility and security tests.

## Evidence ownership and phase gates

P04 is complete after implementation, deterministic account-policy/error tests and at least one real positive own-client authentication. G0 additionally needs source/runtime availability probes. Real no-access, owned and subscription tests remain required A03–A06 evidence for P13/G5; missing tests do not retroactively make P04 incomplete but do prevent full qualification. P09 has no auth bypass or dependency exception.

Earlier phases can be marked complete on their documented host-level checks; this never implies full platform qualification. P12 checks packaging and clean-machine installation for claimed package targets. P13 completes the game/auth/security matrix and focused smoke tests of rebuilt final artifacts. Record every blocked acceptance item instead of shrinking the list. The specification audit in version 1.2.0 does not count as implementation or application test evidence.

## Required cross-version and failure scenarios

The cases MV01–MV12 in [document 10](10-MULTIVERSION-AND-CACHE.md) refine A09–A20 and A22–A24 without replacing the existing matrix. P05/P06/P07 own the pure cache/plan/runtime cases; P08 owns installation/recovery/repair; P09/P10 own launch/cancellation/background lifecycle; P13 records final integration evidence. Synthetic metadata proves decision behavior only; actual baseline installs/launches still need real evidence. Measure reused network bytes separately from disk allocation; do not claim space savings merely because URLs were deduplicated.
