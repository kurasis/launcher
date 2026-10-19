# 06 — Dedicated server installation and lifecycle

## Support tuple

A support tuple consists of game version, runtime flavor, loader family/build, server artifact digest, client generation, Java build/architecture, identity adapter version and OS. P01 chooses one exact baseline from the base launcher's genuinely supported client versions. Do not select a moving latest version at every startup. Additional versions use distinct installation locks and worlds.

The first candidate is the vanilla distribution. If the product's required pre-play identity hook needs a bridge or loader bootstrap, that minimal dependency is implemented and qualified in P06/P07 as part of the first honest support tuple. Do not defer a core identity prerequisite until P13/P14. General pack management and additional advertised loader tuples remain later milestones.

## Source resolution

Use the official game version catalog to fetch the selected descriptor, then its server download descriptor. Validate source hashes and sizes, store the descriptor bytes and record a local SHA-256. If a selected old version lacks a server artifact, report unsupported; never guess a URL. Use the existing managed-runtime provider and exact runtime lock. Sources and retrieval status are in [SOURCES](SOURCES.md).

The adapter compiles the correct launch plan for that specific distribution. Newer bundled server JARs and older distribution formats are not assumed to have identical layouts. Initial tests must prove library extraction/runtime behavior under an isolated working directory.

Installer and bootstrap programs are executable code. Obtain them only through reviewed source policies, pin their bytes, execute them with a constrained environment and inspect resulting files before marking installation complete. Do not execute arbitrary post-install scripts from imported room manifests.

## ServerAdapter contract

Methods: `resolve(planInputs)`, `validateInstall(lock)`, `compileLaunch(lock, worldPath, policy)`, `probeReady(process)`, `requestSave(process)`, `requestStop(process)`, `observePlayers(process)`, `collectHealth(process)` and `validateIdentityCapability()`.

Declare capabilities rather than fake them: ready probe, save barrier, reliable player identity events, tick metrics, remote management, snapshot safety and runtime flavor. Unavailable metrics display unknown. Version-specific console parsing is tested against fixtures and a real session.

## Start transaction

Production room start:

1. Check current account permission, local device consent and the absence of pending world maintenance. Prepare/download the exact install and obtain required content/terms consent before holding a live host lease.
2. Request a room-level start operation with expected room, owner and placement revisions. The assigned host claims it and transactionally allocates one session/host epoch. A repeated command resolves to the same operation; no session ID is required to request start.
3. Acquire resource, instance and world reservations in [05](05-STORAGE-AND-CONCURRENCY.md) order. If acquisition fails, mark the session attempt failed and release its authority; do not wait indefinitely while holding conflicting locks.
4. Verify the selected world receipt and exact install, register a privacy-scoped transport endpoint, and renew the host lease throughout bounded preparation/startup. An expired lease cannot be revived.
5. Prepare local-only services and the pre-play identity bridge, then persist spawn intent, spawn without a shell and record process identity.
6. Drain output using the log policy; require adapter ready probes, identity capability and matching backend/local world publication.
7. Publish ready for the same authority/session/host epoch and placement revision; only then accept gameplay admissions.

Qualification and validation purposes are separate non-public operations. P06 can launch a real server in an isolated test directory before P07 production admission exists. It must not publish a room, accept remote guests or count as a successful identity test. P09 restore validation uses a throwaway clone and a maintenance permit, never the normal ready-room path. These modes are unavailable to an untrusted frontend or guest.

Default Java startup deadline is 180 seconds from spawn; qualified heavy packs may declare up to 600 seconds. Download time, user consent, waiting for a host and pre-play identity time have distinct deadlines/states. Cancellation, lease loss or process exit interrupts startup. No timeout authorizes killing an unrelated process.

New-world inputs are typed: name, optional seed, supported preset, difficulty, rules and player limit. Validate version-specific values and record resolved generation parameters where obtainable. Do not inject arbitrary property lines or silently alter existing chunks when generation settings change.

## Endpoint restrictions

Game listener defaults to loopback. Agent maps an opaque service ID to its fixed local server destination. Guests cannot select destination host/port. Management endpoints stay loopback or on authenticated local IPC; do not expose them through the gameplay relay. Reserve/probe ports safely and handle a bind race by failing and retrying a new validated launch, not by terminating the process already using the port.

## Stop

Close new admissions, notify participants, issue graceful stop and wait for confirmed Java exit, then drain bounded logs and update receipts. For an ordinary stop, release reservations after finalization. For snapshot/restore/handoff, the lifecycle actor transfers the stopped process state into the already-authorized maintenance operation and retains the same world reservation until its terminal boundary; no unlock/relock gap is allowed. Default stop grace is 60 seconds, configurable to 300 seconds for qualified packs. If it expires, show unresponsive status and require a deliberate force-stop action. Retain the world lock while the process lives. A forced stop produces an unclean receipt and cannot be presented as a clean backup.

Host-lease expiry immediately closes guest tunnels and starts graceful stop, but a slow/unresponsive process still retains its local world lock. No successor host may start from an unconfirmed final snapshot. The backend may show unavailable rather than pretending a stopped world is available.

## Runtime lifecycle

Closing the UI leaves the agent and hosted server running. Closing the host's game client does not stop the server. Explicit Quit while hosting offers leave agent running or save-and-stop. Session logout follows [03](03-OAUTH-AND-IDENTITY.md). Idle shutdown is opt-in with a visible delay and recheck of occupancy; unknown occupancy cannot be treated as empty.

Never pass broad inherited Java injection variables or arbitrary frontend JVM arguments. Preserve only the adapter's allowlisted environment and typed advanced settings. Log failure must not block process pipe drainage.

## Start cancellation boundary

Use the start-operation cancellation and room executionRevision contract in [09](09-API-AND-IPC.md). Check the durable cancellation flag at every asynchronous boundary, especially after downloads/reservations and immediately before spawn. If spawn wins the race, complete a controlled stop and report its real outcome. A cancelled preflight must not create a delayed start operation. The local resource owner can always request stop during a control outage; this does not authorize publication, export or a new world transition.

New-room preflight first completes initialize-world publication from [18](18-AUTHORITY-AND-MAINTENANCE.md). It must not use the gameplay start transaction to initialize a missing active pointer. The initializer has bounded supervision, isolated listeners, the same executable-consent rules and a durable process receipt; its world generation is intentional and is not discarded as a validation tick.

The adapter also owns the qualified direct-file logging/crash-output configuration from [13](13-SECURITY.md). Bounded pipe drainage alone does not satisfy the logging requirement; verify the actual run directory and snapshot exclusions.
