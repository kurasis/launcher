# 05 — Storage, immutable generations and concurrency

## Local layout

Resolve DATA_ROOT through the application's platform path implementation. IDs in paths are generated validated identifiers, never display names.

| Relative path | Purpose |
| --- | --- |
| `wwf/agent.sqlite` | Local jobs, projections, leases and process receipts; no plaintext secrets |
| `wwf/rooms/<roomId>/room-cache.json` | Non-authoritative room snapshot |
| `wwf/servers/<instanceId>/install.lock.json` | Exact immutable server plan receipt |
| `wwf/servers/<instanceId>/generations/<generationId>/` | Verified managed server installation |
| `wwf/worlds/<worldId>/generations/<worldGenerationId>/working/` | Mutable working directory for one isolated generation; selected only by world.json |
| `wwf/worlds/<worldId>/world.json` | Lineage and active authority receipt |
| `wwf/snapshots/<snapshotId>/` | Immutable snapshot payload, manifest and completion receipt |
| `wwf/runs/<sessionId>/` | Process receipts, restricted launch files and isolated runtime outputs |
| `wwf/jobs/<jobId>/` | Staging and resumable operation receipts |
| `wwf/client-profiles/<accountId>/<roomId>/<pack-sha256-hex>/` | Account-isolated room profile with target-specific install lock; personal settings are separate |
| `wwf/logs/` | Bounded redacted logs |
| `wwf/locks/` | OS-backed lock handles and non-authoritative diagnostics |
| `shared/wwf/objects/sha256/<prefix>/<digest>/blob` | New content-addressed verified artifacts |

Reuse the base launcher's verified cache through an adapter; do not duplicate bytes just to change directory naming. Upstream hashes remain source-integrity evidence, while locally computed SHA-256 supplies consistent internal object identity. Hash equality establishes byte equality, not trust in the publisher. Runtime identity includes provider, OS, CPU architecture, exact build, image type and archive digest.

Game/server tools that need writable Maven layouts receive isolated projections or copies. Never hardlink writable mod/config files into the shared cache. A read-only bit alone is not a robust isolation mechanism on all platforms. Reflinks are optional; correctness must not depend on their availability.

## Lock ordering and lifetime

One agent owns the OS-backed DATA_ROOT lock for its lifetime. The legacy supervisor must surrender ownership through a stopped migration; two processes never write the same root.

Task locks are acquired in order: resource reservation, sorted instance locks, sorted world locks, sorted artifact publication locks. Never keep a database write transaction open across hashing, downloads, process waits or network requests. Download publication uses short artifact locks; active immutable artifacts use pin references, not one global long-held cache lock.

The per-world lifecycle actor owns the exclusive world reservation and server instance lock. Java uses that reservation; a snapshot request is serialized by the same actor, which stops its current child and retains the lock through snapshot/restore. It MUST NOT first wait for a second non-reentrant world lock while the child waits for that snapshot request to issue stop. A game retains its separate client instance lock. Different instances can run concurrently; two processes cannot open one mutable world. Resource reservations follow the global acquisition order. A disposable validation clone is a private child of the maintenance actor's already-owned operation custody, not a separately exposed instance whose lower-ranked lock is acquired after the world lock. Reserve any additional memory before entering the maintenance transaction, or use a non-blocking budget check that fails cleanly; never wait on lower-ranked resources while holding the world reservation. Cache GC cannot delete a pinned object or a referenced incomplete operation. Core GC may be conservative.

## Transactional installation

Resolve and persist an exact plan before downloading. Freeze source URLs, expected digests, platform, server/loader build and runtime. Download to owned `.part` staging files, verify, publish immutable objects atomically, construct an isolated generation, validate it, then atomically commit the lock. Only then update derived UI summaries.

A retry reuses the frozen plan. Repair reinstates the pinned generation, never resolves a new latest build. Core repair of a corrupted cache object referenced by a running process is deferred until every affected consumer stops. Do not invent alternate locations for the same frozen lock during gameplay. Invalidate dependent health, preserve pins and never overwrite an open shared object in place.

App-owned JSON has schemaVersion. Unknown newer schemas block mutation. Backend uses versioned transactional database migrations. Preserve original upstream metadata bytes separately from parsed receipts. Local database corruption must preserve the original file, stop automatic writes and offer reconstruction from validated locks and world receipts.

## Resource reservation

Reserve expected memory for the game client, server, agent and OS headroom before spawning. Heap size is not total process RSS. Show the planned budget, allow user adjustment within configured limits and refuse clearly impossible budgets. Do not hardcode one universal heap amount for all packs.

Throttle hashing/downloads while gameplay is active. Bound Java stdout/stderr processing even if disk logging fails. Monitor low disk space and warn early; do not blindly kill Java or remove live files to free space.

## Crash windows

Persist spawn intent before process creation, then PID plus process start identity, executable identity and session epoch. If the agent crashes before recording the PID, reconcile conservatively. Never kill by PID alone. An orphan process with uncertain ownership blocks conflicting operations until safely resolved.

Publish snapshots and installations with staging and atomic completion records. Check Windows replacement behavior explicitly; deleting the prior good file before rename is not atomic recovery. Cancellation before commit removes only owned staging; cancellation after commit reports the committed result.

## Durable world pointer and crash ownership

world.json contains worldId, activeGenerationId, placementRevision, lastMutationId and any pending transaction, and resolves only to an existing validated directory under generations. Do not implement the active pointer using an unsafe delete/rename sequence or an unvalidated symlink. Backend publication uses the explicit commit protocol in [18](18-AUTHORITY-AND-MAINTENANCE.md); local filesystem replacement and a database transaction are not one atomic operation.

An OS lock held by the agent is released if the agent dies even when Java survives. Therefore lock-handle lifetime alone cannot enforce child lifetime. Keep durable per-run reservations and reconcile all run receipts before allowing any new writers, GC or snapshots. Uncertain ownership blocks mutation. An authenticated process keeper may retain handles across agent restart, but is not assumed to exist.

Pins include active and uncertain surviving processes, restore candidates, snapshots and handoffs. Reconstruct them conservatively from receipts before enabling cleanup. Never infer that a missing UI, lost IPC connection or free lock file means a child has exited. Recovery options and lost process pipes are specified in [18](18-AUTHORITY-AND-MAINTENANCE.md).

Filesystem digest components are raw lowercase hexadecimal, never the wire-format `sha256:` prefix containing a colon. UUID/path components are validated separately from display strings. The same manifest must materialize safely on Windows and Unix targets.

Long hashing, archive work and pipe drains run through bounded workers without blocking lease watchdogs/control capacity. Never hold a local database write transaction across those jobs. Large-world disk/CPU stress is part of lease qualification, not only a throughput benchmark.
