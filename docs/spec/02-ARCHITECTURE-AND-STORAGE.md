# 02 — Architecture, repository and persistent storage

## Process boundaries

The React webview renders UI and sends typed intentions. The Rust core owns authoritative state, OAuth, network requests, credentials, installation planning, downloads, integrity checks and Java processes. Minecraft runs as a supervised child process. The webview is disposable: closing or recreating it must not cancel a game, lose logs, or erase a running job.

No Node.js runtime or local application server is shipped. Vite is a development/build tool. The only local HTTP listener is the short-lived OAuth loopback callback. Production UI assets come from the packaged application, never a remote website.

## Repository layout

Use these directories (individual file names may be refined):

| Path | Responsibility |
| --- | --- |
| `src/app/` | Routing, top-level layout, theme and bootstrap |
| `src/features/accounts/` | Login, profile and access states |
| `src/features/instances/` | Installation list, create form, play actions |
| `src/features/downloads/` | Progress and repair UI |
| `src/features/settings/` | Validated settings |
| `src/features/diagnostics/` | Logs and error views |
| `src/components/` | Reusable accessible UI components |
| `src/styles/` | CSS variables, typography, spacing and theme tokens |
| `src/lib/ipc.ts` | Single typed wrapper for Tauri invoke/events |
| `src/lib/contracts.ts` | Frontend types generated from or checked against Rust DTOs |
| `src-tauri/src/commands/` | Small IPC handlers; validation and delegation only |
| `src-tauri/src/auth/` | OAuth, Xbox, Minecraft, entitlement adapter, secret store |
| `src-tauri/src/catalog/` | Manifest parsing and supported-version filtering |
| `src-tauri/src/install/` | Resolver, rules engine, download plan, executor, verification |
| `src-tauri/src/runtime/` | Adoptium resolver, extraction, runtime verification |
| `src-tauri/src/launch/` | Argument compiler, child supervisor, redaction |
| `src-tauri/src/storage/` | Paths, atomic JSON, schema migrations, locks |
| `src-tauri/src/lifecycle/` | Window recreation, tray, single-instance and shutdown policy |
| `src-tauri/capabilities/` | Narrow window/plugin permissions |
| `tests/fixtures/` | Minimal public metadata and synthetic non-secret responses |
| `docs/` | Decisions, sources, setup, verification and troubleshooting |
| `.github/workflows/` | Build/test/package matrices |

React with CSS Modules and CSS variables is the default styling choice; Tailwind may be substituted if it keeps all visual tokens centralized. Do not add a large UI framework just for a few forms. A small component library is acceptable when accessible keyboard behavior is reused.

## Rust implementation building blocks

Use established crates for async I/O, HTTP/TLS, serialization, hashing, archive handling, URL parsing, OAuth PKCE, random values, credential vault integration and structured logging. Candidates include `tokio`, `reqwest`, `serde`, `serde_json`, `sha1`, `sha2`, `zip`, `tar`, `flate2`, `url`, `oauth2`, `keyring`, `tracing`, `thiserror` and `uuid`. Select compatible current versions and necessary feature flags at implementation time. This is a responsibility list, not permission to use obsolete examples or copy every candidate dependency.

Own interfaces: `AuthProvider`, `SecretStore`, `ManifestSource`, `RuntimeProvider`, `DownloadTransport`, `InstallPlanner`, `ProcessSupervisor`. Production implementations are fixed official providers; alternate adapters are for tests, not a user-configurable auth bypass.

Use a Rust application state object with bounded channels and short-lived locks. Do not hold mutex guards across network requests. Blocking archive/hash work runs outside the UI/event-loop thread. Task cancellation is explicit, not an accidental consequence of dropping a webview subscription.

## Data directories

Resolve the application-local data path using Tauri's path API. With a provisional reverse-domain identifier `com.example.launcher`, typical bases are `%LOCALAPPDATA%/<app-id>` on Windows, `~/Library/Application Support/<app-id>` on macOS, and `$XDG_DATA_HOME/<app-id>` or `~/.local/share/<app-id>` on Linux. Actual returned OS paths are authoritative; do not concatenate these examples blindly.

Call this path `DATA_ROOT`. Use it as follows:

| Relative path | Contents / ownership |
| --- | --- |
| `settings.json` | Versioned non-secret settings and selected instance |
| `accounts.json` | Active profile ID/name and vault lookup key; no tokens |
| `compatibility.json` | Installed copy of shipped support policy |
| `metadata/catalog.json` | Original cached upstream catalog bytes |
| `metadata/catalog.receipt.json` | App-owned schema, fetch time and HTTP validators |
| `metadata/versions/<version>/<manifest-sha1>.json` | Immutable validated version descriptors |
| `shared/clients/<client-sha1>/client.jar` | One canonical game-client object per SHA-1, independent of version ID |
| `shared/libraries/<artifact-sha1>/artifact.jar` | One canonical library/native JAR per SHA-1; original Maven name/path retained in the lock |
| `shared/assets/indexes/<index-id>.json` | Asset index projection expected by the game |
| `metadata/asset-indexes/<index-sha1>.json` | Immutable validated asset-index metadata |
| `shared/assets/objects/<first-two-hash-chars>/<sha1>` | Content-addressed asset objects |
| `shared/log-configs/<sha1>/<id>` | Verified logging configuration |
| `runtimes/temurin/<os>-<arch>/<major>/<image-type>/<archive-sha256>/` | Private runtime; image type is jre or jdk, release name retained in receipt |
| `instances/<instance-uuid>/instance.json` | User-facing instance definition |
| `instances/<instance-uuid>/install.lock.json` | Exact resolved inputs and verified files |
| `instances/<instance-uuid>/game/` | Worlds, options, screenshots, game logs and user data |
| `instances/<instance-uuid>/runs/<run-uuid>/natives/` | Isolated native extraction paths |
| `instances/<instance-uuid>/runs/<run-uuid>/launch.args` | Optional restricted Java argfile, never exported |
| `jobs/<job-uuid>.json` | Restartable non-secret download/verification job receipt |
| `downloads/<job-uuid>/<algorithm>-<digest>.part` | Job-owned uncommitted downloads; never reused without verification |
| `logs/launcher/` and `logs/runs/<run-uuid>/` | Redacted rotating diagnostics |
| `locks/` | Local process/instance lock records |

Keep managed files and user game data separate. Repair may only rewrite managed artifacts listed in a plan; it must not remove or replace `saves`, `options.txt`, screenshots, resource packs or game-created files. Do not touch the official launcher's account databases or token stores.

## Persistent models

App-owned JSON documents have `schemaVersion: 1`. Upstream catalog, version descriptors and asset-index JSON remain byte-for-byte unchanged; their hashes cover the original bytes, not reserialized objects. Store app metadata in separate receipts. App-owned mutations are validated, written to a temporary sibling, flushed and atomically replaced using a platform-tested replacement primitive. On Windows, do not assume POSIX rename-over-existing behavior or delete the prior file before replacement. Recover interrupted promotion from the last-good file/receipt. Keep one last-good settings backup. On a newer unknown schema, stop mutation and explain version incompatibility. On corrupt settings, preserve the corrupt file for diagnosis and offer defaults; never silently erase worlds.

An `Instance` contains: generated UUID; display name (1–64 characters); version ID; nullable manifest SHA-1 until initial resolution; created/last-played timestamps; maximum heap MiB; resolution; installation health and active operation reference; last result. Version is immutable for MVP; create a new instance for a different version to avoid world upgrades/downgrades.

An `InstallLock` contains: schema version; instance ID; version descriptor URL/hash; Java major; OS and effective JVM architecture; runtime provider/build/archive SHA-256 and extracted-tree receipt identity; ordered classpath entries; asset index ID/hash; logging config; every required artifact's relative path, size, hash algorithm/hash and source URL; validation times; completion flag. No tokens, user password, OAuth code, full launch command or sensitive argfile contents.

A newly created instance is a draft (`not_installed`, `manifestSha1: null`); P02 does not invent a descriptor hash. Initial resolution pins a verified hash. The instance is `ready` only after its complete lock with non-null hashes has been committed. A hash directory plus a completion marker identifies a usable shared artifact; directory existence alone does not.

## Concurrency and recovery

- One application supervisor per OS user and application identity; subsequent app launches recreate/focus the existing GUI.
- One global activity permit covers installation/repair OR launch preflight/running game/cleanup. Retain instance locks too. Conflicting operations on any instance return `APP_BUSY`; a second game returns `GAME_ALREADY_RUNNING`. Do not mutate shared game artifacts or runtimes while any game, including a detected surviving orphan, is alive. Read-only browsing and unrelated UI settings remain available.
- Deduplicate concurrent artifact downloads by verified content identity.
- Hold a process/instance lock while a game is running, including when GUI is absent.
- On restart, reconcile incomplete jobs and `.part` files; reuse verified completed artifacts. Unverified bytes never become valid simply because a receipt exists.
- Persist a non-secret active-run record with PID, process start identity and instance. If the launcher crashed while Java survived, block a second run until that process is confirmed gone or the record is proven stale. Never identify a process by PID alone and kill it.
- Asset index IDs are not immutable content identities. Two instances may pin the same index ID with different hashes. Under the global activity permit, immediately before spawn, atomically project the selected immutable hash into the game-facing index path. No install/repair may change it while the game runs. Test alternating these two instances.
- Garbage collection, migrations between root directories and deletion of installations are deferred. Temporary staging cleanup must be constrained to this app's own non-active job directories.

## Lock and recovery invariants

Acquire the global activity permit before instance/shared-file locks; never reverse that order. A preflight that needs Repair releases its permit and reports `REPAIR_REQUIRED`; it does not call a competing repair operation while holding the permit. Release a run permit only after Java exit, bounded pipe drain and managed run cleanup. Never hold a mutex guard while waiting for a child or network.

Persist a launch-intent receipt before spawn, then persist PID plus process-start identity immediately after spawn. A crash between these steps leaves an ambiguous launch; startup must reconcile conservatively and block conflicting writes/new runs until it can establish that no previous game uses the instance. A stale PID alone is not sufficient evidence. Record interrupted operations explicitly; do not convert them to success.

The shipped compatibility policy is application-controlled. A DATA_ROOT copy is a cache of that policy, not a frontend-editable authorization source. Newer unknown app schemas are read-only until a supported migration exists.

## Cross-version cache and commit authority

Use the exact reuse and regression contract in [10 — Multi-version and cache](10-MULTIVERSION-AND-CACHE.md). Instance identity, version metadata, artifact content identity and Java runtime identity are distinct keys. A Maven coordinate or Java release label alone never identifies verified content.

Use an OS-backed exclusive lock on DATA_ROOT for the supervisor's lifetime before any recovery or managed write. A PID file or Tauri single-instance notification alone is not the storage lock. A second process must fail closed if the lock is held, even if it cannot contact/focus the first process (for example another desktop session). Release the handle only on supervisor exit; never delete a lock file to override a held lock.

`install.lock.json` is the authoritative completed installation generation. Write and verify artifacts/receipts first, then atomically commit the complete lock, then update the derived instance summary. A crash after lock commit but before instance.json update is reconciled from that lock; a summary saying ready without a valid lock is not trusted. Interrupted verification remains distinct from complete installation health. Reusing existing ready content does not require deleting a prior valid lock.

For a new installation persist the resolved descriptor/runtime plan in the job receipt before bulk downloads. Retry uses that frozen plan; no new latest-runtime lookup on each retry. Before a plan has been committed, a failed resolution may be retried normally. Existing committed install locks always take precedence over an interrupted job's candidate plan.
