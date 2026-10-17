# Decision records

Significant implementation decisions and deviations from the specification
([docs/spec/](spec/README.md)). Each entry states the reason, the affected
requirement identifiers and how it is verified. Newest entries are appended at the end.

Status values: `accepted` (in effect), `provisional` (in effect until the owner confirms or
replaces it), `superseded`.

---

## D-001 — Provisional application identity

- **Status:** superseded by [D-031](#d-031--neutral-application-identity)
- **Phase:** P01 · **Affects:** FR13, A24, A26; DATA_ROOT, credential-vault and single-instance namespaces
- **Decision:** A provisional product name, bundle identifier and binary name derived
  from the repository owner. The name contains no
  Minecraft or Mojang branding. The UI shows a "Development build" tag.
- **Reason:** The specification asks for a clearly provisional identity until the owner
  chooses one. A reverse-domain identifier based on the GitHub account avoids collisions.
- **Consequence:** Changing the identifier after users have data requires an explicit
  migration plan (doc 07). It is cheap to change before any package is distributed.
- **Verification:** `src-tauri/tauri.conf.json`; release build window title (docs/VERIFICATION.md).

## D-002 — Toolchain and dependency pins

- **Status:** accepted · **Phase:** P01 · **Affects:** A25
- **Decision:**
  - Node.js 24.21.0 LTS (`.nvmrc`), npm with a committed `package-lock.json`; direct
    dependencies use exact versions.
  - Rust 1.97.0 via `rust-toolchain.toml`; `Cargo.lock` committed; Rust edition 2024.
  - TypeScript 6.0.3, not 7.0.x: the current `typescript-eslint` (8.71.2) supports
    TypeScript `>=4.8.4 <6.1.0` only. Revisit when typescript-eslint supports TS 7.
  - ESLint 10 without `eslint-plugin-jsx-a11y`, whose latest release (6.10.2) does not
    declare ESLint 10 support. Accessibility is covered by role/name-based component tests
    and keyboard/focus styling; re-add the plugin when it supports ESLint 10.
  - Node 22 was available on the host but `jsdom` 30 requires Node `^22.22.2 || ^24.15.0`;
    Node 24 is the current LTS line.
- **Verification:** `npm ci` produces no engine warnings on Node 24.21.0; CI uses `.nvmrc`
  and `--locked`.

## D-003 — Styling approach

- **Status:** accepted · **Phase:** P01 · **Affects:** doc 06 UI direction
- **Decision:** CSS Modules plus CSS custom properties in `src/styles/tokens.css` (the
  specification's default). No UI framework. System font stack; light/dark follows the
  system preference; `prefers-reduced-motion` disables transitions. User-facing strings live
  in `src/lib/strings.ts` for later localization.

## D-004 — Microsoft client ID is build-time, non-secret configuration

- **Status:** accepted · **Phase:** P01 (configuration only; auth flow is P04) · **Affects:** FR01, FR03, G0, A03
- **Decision:** The Rust core reads `LAUNCHER_MSA_CLIENT_ID` at compile time
  (`option_env!`). The value must be a canonical GUID; the nil GUID is rejected. There is
  no default or fallback ID. The UI receives only a status: `not_configured`, `invalid` or
  `configured_unverified`. Nothing reports the ID as verified until a real sign-in has
  succeeded with it (P04).
- **Reason:** A public client has no secret, so the ID may later be committed once the
  owner approves it; until then builds without it remain fully functional for non-auth work.
  Never borrowing another launcher's ID is a hard rule (START-HERE.md item 6, doc 03).
- **Verification:** `src-tauri/src/auth/client_id.rs` unit tests; UI tests in
  `src/app/App.test.tsx`.

## D-005 — Explicit per-command IPC permissions

- **Status:** accepted · **Phase:** P01 · **Affects:** doc 07 trust boundaries, A07, A21
- **Decision:** `build.rs` declares every app command in a Tauri `AppManifest`, which turns
  each command into an `allow-<command>` permission. The main window capability grants
  only those permissions — no `core:default`, no plugins (no shell, fs, http or opener).
  `src-tauri/tests/security_config.rs` pins the exact permission list, so a new grant is a
  reviewed change. Plugins such as the opener are added in the phase that needs them, with
  Rust-side URL allowlisting.
- **Verification:** `security_config.rs`; the release build renders data from
  `get_app_info` with only this permission (docs/VERIFICATION.md).

## D-006 — Content Security Policy

- **Status:** accepted · **Phase:** P01 · **Affects:** doc 07
- **Decision:** Production CSP: `default-src 'self'`, `script-src 'self'`, `style-src 'self'`,
  `img-src 'self' data:`, `connect-src ipc: http://ipc.localhost`, `object-src 'none'`,
  `base-uri 'none'`, `form-action 'none'`, `frame-ancestors 'none'`; Tauri adds hashes for
  its own injected scripts. A separate `devCsp` additionally allows inline scripts/styles
  and `ws://localhost:1420`, which Vite's React refresh and HMR require in development only.
  `freezePrototype` is enabled.
- **Verification:** `security_config.rs` rejects remote sources, `unsafe-eval` and
  wildcards; release build renders correctly under the production CSP.

## D-007 — CI matrix and scope

- **Status:** accepted · **Phase:** P01 · **Affects:** FR13, A25
- **Decision:** `.github/workflows/ci.yml` runs frontend checks on `ubuntu-24.04` and, on
  `ubuntu-24.04`, `windows-2025`, `macos-15` (arm64) and `macos-15-intel` (x64): clippy with
  `-D warnings`, `cargo test --locked` and a release build without installers
  (`tauri build --no-bundle`). Rustfmt runs once on Linux. The workflow has read-only
  permissions and uses no secrets. Installer packaging and artifact upload belong to P12.
- **Limitation:** A CI build is compile evidence, not a target-machine test.

## D-008 — Declared bundle targets

- **Status:** provisional (validated in P12) · **Phase:** P01 · **Affects:** FR13, A01, A02
- **Decision:** `bundle.targets` is `deb`, `appimage`, `nsis`, `app`, `dmg`, matching doc 07.
  MSI and RPM are not produced. P01 built a `.deb` on the host only to confirm the bundler
  works; packaging is not qualified.
- **Open item for P12:** doc 07 asks to build Linux packages on the oldest qualified
  baseline. Debian 12 has an older glibc (2.36) than Ubuntu 24.04 (2.39), so a binary
  built on Ubuntu 24.04 may not start on Debian 12. P12 must build Linux packages on a
  Debian 12-compatible baseline and test both distributions.

## D-009 — Repository bootstrap

- **Status:** accepted · **Phase:** P01
- **Decision:** The repository was empty (no commits or branches). `main` was created with
  one commit that imports the unmodified specification into `docs/spec/`, so every phase,
  starting with P01, can be reviewed as a pull request against it.

## D-010 — Project license not chosen

- **Status:** open (owner decision) · **Phase:** P01 · **Affects:** About screen (doc 06), P14 handover
- **Decision:** No `LICENSE` file is added. Choosing the project's license is the owner's
  decision. Third-party dependency notices are collected before distribution (P12/P14).

## D-011 — Windows application manifest embedded for every target

- **Status:** accepted · **Phase:** P01 · **Affects:** A25 (Windows CI)
- **Decision:** `build.rs` disables tauri-build's own manifest and, on Windows MSVC, embeds
  the identical Common Controls v6 manifest (`src-tauri/windows-app-manifest.xml`) through
  linker arguments for all targets, including test executables.
- **Reason:** tauri-build adds its manifest only to the binary target. Rust integration test
  executables that link Tauri then fail on Windows with `STATUS_ENTRYPOINT_NOT_FOUND`
  (0xc0000139), observed in the first CI run of PR #1.
- **Verification:** Windows CI job runs `cargo test` and the release build.

## D-012 — Application state, revisions and events

- **Status:** accepted · **Phase:** P02 · **Affects:** doc 06 bootstrap, A21
- **Decision:** `AppState` in Rust is the only authority. Each mutating command takes a
  `MutationRequest { requestId, payload }`, persists its change before it becomes visible,
  then advances the monotonic `revision` by one per emitted event (`instance_changed`,
  `settings_changed`). Snapshots carry `sessionEpoch` (a UUID per backend process) and
  `revision`. The frontend store subscribes before fetching the snapshot, buffers events,
  applies only `revision + 1`, ignores older revisions and refetches on gaps or a new epoch.
  After a successful command the frontend also refreshes the snapshot, so a lost event
  costs one extra read instead of stale UI.
- **Idempotency:** the last 512 request IDs per epoch are remembered with a canonical
  fingerprint of the typed payload; identical replays return the recorded result (success
  or error) and different payloads return `REQUEST_ID_CONFLICT`.
- **Verification:** `src-tauri/src/state/tests.rs`, `src/lib/store.test.ts`, `src/lib/ipc.test.ts`.

## D-013 — Error codes added to the doc 06 vocabulary

- **Status:** accepted · **Phase:** P02 · **Affects:** doc 06 error vocabulary
- **Decision:** In addition to the required codes, the backend uses `AUTH_NOT_CONFIGURED`,
  `INSTANCE_NOT_FOUND`, `OPERATION_NOT_FOUND`, `INVALID_INPUT`, `INSUFFICIENT_MEMORY`,
  `DATA_ROOT_LOCKED`, `STORAGE_UNAVAILABLE`, `STORAGE_ERROR` and `INTERNAL`. Doc 06 lists a
  minimum set; none of these replaces a required code.

## D-014 — DATA_ROOT lifetime lock and debug-only relocation

- **Status:** accepted · **Phase:** P02 · **Affects:** doc 02, doc 10 MV09, A16
- **Decision:** The supervisor takes an exclusive OS file lock on
  `DATA_ROOT/locks/supervisor.lock` (Rust `File::try_lock`: `flock` on Unix, `LockFileEx` on
  Windows) before any recovery or write, and holds it for its lifetime. A second process
  starts read-only with `DATA_ROOT_LOCKED` shown and never writes. The OS releases the lock
  on exit or crash; the lock file is never deleted. P03 adds the single-instance handoff
  on top of this lock, not instead of it.
- **Relocation:** debug builds honour `LAUNCHER_DATA_ROOT` for tests and demonstrations.
  Release builds ignore it; DATA_ROOT relocation stays out of MVP scope (doc 02).
- **Path policy:** DATA_ROOT must be absolute, valid Unicode and free of the platform
  classpath separator (`:` on Unix, `;` on Windows), otherwise `UNSUPPORTED_DATA_PATH` /
  `UNSUPPORTED_PATH_ENCODING` is reported before any write. Spaces and non-ASCII text are
  allowed and exercised by tests and the P02 demonstration.

## D-015 — Persistent document handling

- **Status:** accepted · **Phase:** P02 · **Affects:** doc 02 persistent models, MV08
- **Decision:** App-owned documents carry `schemaVersion: 1` and are written through a
  flushed temporary sibling plus `std::fs::rename` (which replaces the destination
  atomically on Linux/macOS and via `MoveFileExW(MOVEFILE_REPLACE_EXISTING)` semantics on
  Windows; the previous file is never deleted first). Stray temporaries are removed at
  startup. A corrupt document is renamed to `<name>.corrupt-<UTC timestamp>` before any
  fallback; settings keep one `settings.json.bak` last-good copy. A newer schema is never
  rewritten and makes that document read-only. Documents above 1 MiB are treated as corrupt.
- **Install lock authority:** `install.lock.json` is the committed installation; the
  instance summary is reconciled from it at startup (a `ready` summary without a complete,
  matching lock becomes `not_installed`; a complete lock with a stale draft summary becomes
  `ready`; an unreadable or foreign lock becomes `repair_required`). The lock schema is
  defined now and filled by P08.
- **Upstream bytes:** catalog, descriptors and asset indexes will be stored byte-for-byte
  under their hash with separate app-owned receipts (P05); they never pass through the
  app-document serializer.
- **Windows verification:** replacement semantics are exercised by unit tests on the
  Windows CI runner; interrupted-replacement behaviour on a real Windows desktop is part of
  P13.

## D-016 — Memory and input bounds

- **Status:** accepted · **Phase:** P02 · **Affects:** FR09, doc 06 bounds
- **Decision:** Implemented exactly as doc 06 specifies: heap 1024 MiB to
  `min(32768, physical − max(2048, 20 %))`; recommended default 4096 MiB on machines with at
  least 8 GiB, otherwise 2048 MiB clamped into range with a low-memory warning; initial heap
  `min(512, max)`. Machines leaving less than 1024 MiB get `INSUFFICIENT_MEMORY` when creating
  an installation. Resolution 640–7680 × 480–4320. Names 1–64 characters without control
  characters; names are never used as paths. Physical memory comes from `sysinfo`.

## D-017 — Window lifecycle: destroy and recreate, Rust-owned supervision

- **Status:** accepted (host-verified on Linux x64 only) · **Phase:** P03 · **Affects:** FR11, A17–A19, A23, doc 05
- **Decision:** In the default `unload_and_return` mode the main `WebviewWindow` is
  destroyed (not hidden) once a supervised run survives a 1.5 s startup interval, and
  recreated from the same window configuration (label `main`, so it receives exactly the
  same capability and packaged URL) when the run ends. In `keep_open` mode the window stays.
  The child handle, output readers and run state live in Rust (`lifecycle::child`,
  `RunHandles`, `AppState`), never in a window.
- **Exit policy:** `RunEvent::ExitRequested` is prevented while the global activity permit
  is held, unless an explicit quit intent was recorded (Quit command after the user's
  choice). Quit intent is never inferred from the exit code. If a window still exists when
  an exit request arrives (application menu Quit, Cmd+Q), the request is prevented, the
  window is focused and the UI shows the Keep / Stop and quit / Cancel choice.
- **Ordering:** every window create/destroy runs on the main event loop via
  `run_on_main_thread`. A delayed unload only proceeds while its run still owns the unload
  token; the token is cleared the moment the child process exits — before the bounded output
  drain — so a window is never destroyed after the process has ended (found and fixed
  during P03 testing with the inherited-pipe scenario).
- **Single instance:** `tauri-plugin-single-instance` 2.5.4 is the first plugin; a second
  launch focuses or recreates the existing window and exits. On Linux it uses the D-Bus
  session bus; if notification fails, the DATA_ROOT lock (D-014) still prevents a second
  writer. macOS Dock reopen uses `RunEvent::Reopen`.
- **Not verified yet:** Windows and macOS window behaviour, macOS menu/Cmd+Q, Wayland. No
  tray icon is implemented; reopening the executable is the background-return path on every
  platform, as doc 05 requires that a tray is never the only path.

## D-018 — Lifecycle test harness only in debug builds

- **Status:** accepted · **Phase:** P03 · **Affects:** doc 05, P03 "test controls"
- **Decision:** The harmless test child is the launcher executable itself started with
  `--launcher-lifecycle-test-child <scenario>`; scenarios are a fixed enum (short, long,
  immediate exit, crash, inherited pipe, heavy output). The child entry point, the
  `start_lifecycle_test`/`stop_lifecycle_test` handlers and the UI panel are compiled only
  with `debug_assertions`; release builds register neither handler, and
  `tests/security_config.rs` checks the release handler list. No input can name a program,
  path or argument.

## D-019 — Pinned Tauri lifecycle APIs

- **Status:** accepted · **Phase:** P03 · **Affects:** doc 05 "pin and test the exit APIs"
- **APIs relied on (Tauri 2.12.4):** `App::run` with `RunEvent::ExitRequested { api }` and
  `ExitRequestApi::prevent_exit`; `RunEvent::Reopen` (macOS); `AppHandle::run_on_main_thread`;
  `WebviewWindowBuilder::from_config` + `build`; `WebviewWindow::destroy`; `AppHandle::exit`;
  `tauri_plugin_single_instance::init`. Programmatic restart is not used. A Tauri upgrade must
  re-run the P03 scenarios in docs/VERIFICATION.md.
- **Child supervision:** stdout and stderr are read on separate threads with 64 KiB line
  and 2,000-line / 2 MiB tail bounds; exit is awaited with a 250 ms `try_wait` interval so an
  explicit Stop can kill exactly the owned child; after exit, pipes are drained for at most
  2 s. Stop never targets processes by name or PID lookup.

## D-020 — Authentication implementation choices

- **Status:** accepted (contract itself is a candidate until G0) · **Phase:** P04 · **Affects:** FR01–FR03, FR15, A03–A08
- **Contract:** documented in [AUTH-CONTRACT.md](AUTH-CONTRACT.md) and pinned in
  `src-tauri/src/auth/contract.rs`. One adapter, no fallback chain to older endpoints.
- **PKCE/state:** generated from the OS random source (`getrandom`), S256 per RFC 7636 (test
  vector from Appendix B), implemented directly instead of through an OAuth crate because
  only the code and refresh grants are needed and every step is covered by tests.
- **HTTP:** `reqwest` 0.13 with rustls and the platform certificate verifier (system trust
  store on every OS, honours system proxies). The auth client is HTTPS-only, follows no
  redirects and is separate from future artifact clients.
- **Vault:** `keyring` 3.6.3 (Windows Credential Manager, macOS Keychain, Linux Secret
  Service). keyring 4.x was not used because it restructured into separate store crates; the
  3.x API is stable and sufficient. No usable vault → explicit session-only mode.
- **Not configured:** without a build-time client ID every sign-in path returns
  `AUTH_NOT_CONFIGURED` and the UI explains it; nothing contacts Microsoft.
- **External links:** the browser is opened only from Rust through `external::check_allowed`
  (HTTPS, exact hosts, no ports or credentials). The webview has no opener permission.
- **XUID:** taken only from the XSTS `xid` claim when present.
- **Verification:** 44 deterministic tests in `src-tauri/src/auth/` against a local fixture
  server; no live service call has been made.

## D-021 — Public source client and metadata cache

- **Status:** accepted · **Phase:** P05 · **Affects:** doc 07 trust boundaries, FR04, A09
- **Client:** `net::PublicClient`, separate from the auth client. HTTPS only; each source
  category (`MojangMetadata`, `MojangArtifacts`, `Adoptium`) has an exact host allowlist that
  every request URL and every redirect hop must match; at most five redirects; no
  credentials; 15 s connect and 60 s read timeouts; responses are size-capped per document.
- **Address policy:** the client's DNS resolver refuses loopback, private, link-local and
  other special-purpose answers for allowlisted names. Any other name the client resolves
  can only be the user's configured proxy (requests cannot target other hosts), so proxies
  on private addresses keep working; the proxy then resolves the destination itself.
- **Catalog cache:** `metadata/catalog.json` plus a receipt (fetch time, ETag,
  Last-Modified, SHA-256). Fresh for 6 hours; a forced refresh revalidates with the stored
  ETag. On an outage the last verified catalog is shown and marked stale; without one, every
  version is shown as "catalog unavailable". Nothing is ever invented.
- **Trust** (superseded by D-035: every catalog version is offered): a version is offered only when it is on the curated compatibility list and the
  catalog lists one of its accepted descriptor SHA-1 values. A changed hash is shown as a
  new candidate and refused for planning. `latest.release` is displayed only as
  information and never adds a version or changes an instance.
- **Descriptors and asset indexes:** verified by SHA-1 (and size) before parsing, stored
  byte-for-byte under `metadata/versions/<id>/<sha1>.json` and
  `metadata/asset-indexes/<sha1>.json`, re-verified on every read and refetched when the
  cached copy is damaged.
- **Planner:** a pure function of descriptor bytes, asset index bytes and a target
  (`RuleContext`). Rules use the last matching action; unknown rule keys, inheritance,
  legacy argument strings, non-release types, unsafe paths, URLs off the allowlist and
  unknown placeholders are rejected. Content paths are canonical by digest (doc 10), so the
  same coordinate with a different digest is distinct content and identical digests are
  shared; a conflicting size for one digest is an error.

## D-022 — Download executor and job receipts

- **Status:** accepted · **Phase:** P06 · **Affects:** FR07, A10, A14, A22, doc 10 MV01/MV03/MV04
- **Transport:** `download::transport::HttpTransport` routes every request through the
  `PublicClient` of its source category (D-021), so the URL, each redirect hop (at most
  five) and the resolved addresses are checked; only `200 OK` is accepted. A test-only
  loopback route exists solely under `cfg(test)` for the fixture suite.
- **Executor:** requests are deduplicated by canonical path; one digest with two sizes,
  or one path with two digests, is `METADATA_CONFLICT`. Existing files are reused only
  after a full size and digest check. Bytes stream to
  `downloads/<job>/<algorithm>-<digest>.part` while hashed and are renamed into the
  canonical path only after size and digest match; the rename is the one step cancel
  does not interrupt. Concurrency 8, stall timeout 60 s, transient failures (connect,
  timeout, short read, 408/429/5xx) retried three times with ±50 % jittered exponential
  backoff or `Retry-After` (a wait above 60 s fails instead), digest mismatch retried once
  from scratch, other 4xx and policy violations never retried. Free space for the missing
  bytes plus a 256 MiB reserve is checked before the first request.
- **Measurements:** the report separates unique committed bytes from all network bytes
  (including failed attempts); progress credits each file with the furthest any attempt
  reached, so it never exceeds the total, and is emitted at most every 100 ms.
- **Receipts:** `jobs/<uuid>.json` is written as `running` before work starts and receives
  exactly one terminal state (the first one wins). On startup, while holding the DATA_ROOT
  lock, running receipts become `interrupted` and `downloads/` is removed; committed
  content is untouched and reused by the next attempt.
- **Scope:** byte-range resume is not implemented (file-level recovery only). The
  executor has no IPC command of its own: no generic frontend download API exists. The
  `operation_progress`/`operation_finished` events and snapshot fields are wired in P08,
  where `install_instance` and `repair_instance` become the first callers.

## D-023 — Managed Java runtime installation

- **Status:** accepted · **Phase:** P07 · **Affects:** FR06, A10, A11, A16, doc 10 MV05
- **Provider:** Adoptium `v3/assets/latest/<major>/hotspot` with exact `os`, `architecture`,
  `image_type=jre`, `vendor=eclipse`. Only `binary.package` (ZIP on Windows, tar.gz on
  Linux/macOS) is used; entries must match vendor, major, OS, architecture, image type,
  HotSpot, normal heap and GA (when a release type is supplied). Package links must pass
  the Adoptium source policy; SHA-256 and a size of at most 512 MiB are required. When no
  JRE exists, a JDK of the same major is chosen and recorded (`jdkFallback`); a different
  major or architecture is never substituted. The resolved `RuntimePackage` is the pinned
  record for job receipts and install locks (P08).
- **Archives:** `tar` 0.4.46 + `flate2` 1.1.10 and `zip` 8.6.0 (9.x was less than two weeks
  old). The extractor normalizes every path, rejects absolute paths, `..`, drive and
  alternate-stream syntax, NUL/control characters, Windows device names, duplicates and
  case collisions, special files, entries below files or links, and links that leave the
  package or dangle; hard links become copies of earlier files. Limits: 20,000 entries,
  512 MiB per file, 1 GiB in total, counted on the bytes actually written. Links are
  created on Unix only (current Windows packages contain none).
- **Tree receipt:** `.launcher-tree.json` lists every directory, file (size, SHA-256,
  executable bit) and link target, sorted by path; its SHA-256 is stored in
  `.launcher-runtime.json` together with the pinned package and the probe result, and goes
  into the install lock as `treeReceiptSha256`. Reuse requires both receipts and every
  file to match; any difference rebuilds the runtime from the pinned archive in fresh
  staging.
- **Probe:** `java -XshowSettings:properties -version` with `JDK_JAVA_OPTIONS`,
  `JAVA_TOOL_OPTIONS`, `_JAVA_OPTIONS` and `CLASSPATH` removed, no shell, no console window
  on Windows, 64 KiB capture per stream, 30 s timeout. `java.specification.version` must
  equal the required major and `os.arch` the target architecture.
- **Promotion:** staging lives under `runtimes/` (same filesystem); an existing directory
  is moved to `.old-<sha256>-<uuid>` until the new one is in place. Startup reconciliation
  removes staging and archives and restores or deletes `.old-*` directories. The archive
  is deleted after installation; Repair downloads the pinned package again.
- **Never touched:** PATH, JAVA_HOME, registry associations, any system or official
  launcher Java.

## D-024 — Install and repair orchestration

- **Status:** accepted · **Phase:** P08 · **Affects:** FR06–FR08, A10, A14, A15, A22, doc 10 MV01/MV06/MV08
- **Operations:** `install_instance`, `repair_instance` and `cancel_operation` return
  promptly; work runs in the background under the global activity permit. A repeated
  request ID returns the same operation; reuse for another request is
  `REQUEST_ID_CONFLICT`. Phases follow doc 06 (`resolving`, `checking_files`,
  `downloading`, `extracting_runtime`, `finalizing`, `cancel_requested`).
- **Frozen plan:** a new install resolves `FinalInstallPlan` (game plan + pinned
  `RuntimePackage`) once and stores it in the job receipt before downloading. Retrying
  after cancel, failure or crash reuses the newest unfinished plan for that instance; a
  changed catalog does not affect it.
- **Commit order:** verified artifacts and runtime tree → asset-index projection →
  `install.lock.json` → instance summary. A cancel after the lock commit reports success.
  Install on an instance with a complete lock runs the repair path.
- **Repair:** re-derives the plan from the locked descriptor URL and hash (never the
  current catalog), requires it to match the locked artifacts and classpath, then fully
  re-verifies every locked artifact and the runtime tree, replacing only what is wrong.
  The lock is not rewritten. Health is `needs_verification` while it runs; a failed repair
  that found damaged shared files marks every installation using them `repair_required`.
  The instance's game directory is never touched.
- **Progress:** `operation_progress` events carry the current revision without advancing
  it and a per-operation sequence; `operation_finished` advances the revision and carries
  the result and the active operation. The snapshot holds the latest progress and the
  last 10 results, so a recreated window recovers state. Interrupted jobs found at
  startup appear as `interrupted` results.
- **Document limits:** install locks and job receipts list every artifact (≈1.6 MB for
  1.21.1) and use a 64 MiB limit; other app documents keep 1 MiB.
- **Runtime reuse** compares package identity (key, URL, size, format), not descriptive
  fields, so a lock-rebuilt package matches the installed one.

## D-025 — Launch compiler, preflight and supervision

- **Status:** accepted (real-game confirmation blocked by P04) · **Phase:** P09 · **Affects:** FR10, A12, A13, A16, A20
- **Preflight** (under the global activity permit): instance `ready`; complete install lock;
  every locked file present with its size (full hashes stay with Repair); runtime tree
  and receipts valid; plan re-derived from the locked descriptor and equal to the lock;
  asset-index projection for this run; fresh account verification
  (`AuthService::verify_for_launch`). Any failure ends the run before a process exists;
  file or runtime problems mark the installation `repair_required` and release the
  permit, so Repair can start immediately.
- **Compiler:** `-Xmx<heap>M`, the metadata JVM arguments in order, the logging argument,
  the main class, the metadata game arguments. Every placeholder needs an explicit value;
  `library_directory` and unknown names are errors. `${natives_directory}/<suffix>`
  directories are created per run. `user_type` is `msa`; `auth_xuid` is the XSTS `xid`
  claim or empty (launch-compatibility candidate, to be confirmed by the first real
  launch). The access token may only appear in game arguments.
- **Arguments on the command line:** direct arguments by default; on Windows JVM arguments
  move to a per-run argfile (`0600` on Unix) when the command would exceed 30,000
  characters. Argfile entries are quoted with `\`/`"` escaping; on Windows they must be
  ASCII. The token is never written to an argfile; the argfile and native folder are
  removed after exit.
- **Encoding guard (A16):** the Java launcher decodes arguments with the native encoding,
  and the live check showed a POSIX locale turning `Jürgen` into `J??rgen`. When any
  non-secret argument is non-ASCII, preflight passes it to the managed Java once as a
  property and refuses the launch with `UNSUPPORTED_PATH_ENCODING` if more `?` come
  back than were sent. This needs no platform-specific code (the crate forbids `unsafe`).
- **Supervision:** `java` is spawned without a shell, with the filtered environment, in
  the instance's `game/` folder; stdout and stderr are captured by the P03 supervisor,
  redacted (including a secret cut by line truncation) and written to
  `logs/runs/<run>/output.log` (10 MiB × 5); write failures switch to
  `loggingDegraded` without blocking the pipes. Completed run logs are pruned to 20 runs
  and 200 MiB. Cancellation is honoured until the spawn (serialized with it); afterwards
  only `stop_game`, which kills the owned process after a UI confirmation, ends it.
- **Window:** the same unload/restore path as the P03 harness (unload after 1.5 s survival
  in the default mode, one window restored with the result).

## D-026 — Background work, quit policy and surviving games

- **Status:** accepted · **Phase:** P10 · **Affects:** FR11, A17–A19, doc 10 rule 6 / MV10
- **Closing the window** while any operation holds the activity permit keeps the
  launcher running (no prompt); a finished install/repair restores one window with its
  result. Explicit Quit asks first: games keep the P03 choice; installs/repairs offer
  "Keep running in background" (default) or "Cancel and quit", which cancels, waits up
  to 30 s for the single terminal record and exits without reopening a window.
- **No tray icon in the MVP.** Reopening the executable (single instance) or the macOS
  Dock restores the window; nothing depends on a tray extension.
- **Surviving games:** the active game's PID, OS process start time and executable are
  stored in `locks/active-run.json`. Only a match of PID, executable and start time
  (within 5 s) is treated as the game. On Linux the start time is kept in seconds since
  boot (`/proc/<pid>/stat` start tick) together with the boot ID, because the start
  time Linux reports as wall-clock time is derived from a boot time that moves with
  every clock correction (NTP, resume); a record from another boot never matches. It is
  watched every 2 s while holding the activity permit and can be stopped by the user
  after the identity is checked again. It is not adopted (no output, no exit code).

## D-027 — Product UI completion and diagnostic export

- **Status:** accepted · **Phase:** P11 · **Affects:** FR09, FR14, FR16, A07, A21, A22
- **Screens:** Installations (list, create, game-files panel with Install/Retry/Repair/
  Verify/Play/Cancel and live progress, details with RAM, resolution, last result and
  Open game folder), Account, Settings (theme, during-game mode, defaults for new
  installations, read-only data location), Diagnostics (last run with output, recent
  operations, export; debug-only lifecycle harness moved here), About (status,
  independent-project notice, licence status, source link, official EULA/help links).
  A header indicator shows the running operation and its percentage on every screen.
- **Open game folder:** the path is resolved in Rust from the instance ID and opened with
  the opener plugin from Rust; the webview has no path or opener permission.
- **Diagnostic export:** `tauri-plugin-dialog` 2.8.1 save dialog (called from Rust only;
  no dialog permission is granted to the webview) and a ZIP built from an explicit file
  list: README, summary, job outcomes and the output logs of the last three runs. Texts
  are re-sanitized (data and home paths shortened; JWT-shaped values and token arguments
  replaced). Account records, tokens, argfiles, install locks and anything from game
  folders are never read.
- **Licence:** none has been chosen by the project owner; About says so instead of
  inventing one.

## D-028 — Packaging, baselines and package tests

- **Status:** accepted · **Phase:** P12 · **Affects:** G5, FR13, A01, A02, A24–A26
- **Targets:** NSIS per-user installer with the silent WebView2 download bootstrapper
  (Windows x64); native `.app`/DMG for macOS arm64 and x64 with minimum macOS 11.0 (a
  provisional floor; P13 sets the final value from the private Java runtimes and real
  machines); `.deb` and AppImage for Linux
  x64. Resolves the open item of D-008: Linux packages are built in a `debian:12`
  container (glibc 2.36), the oldest qualified baseline; the binary needs glibc 2.34.
- **Declared Linux dependencies:** Tauri's generated `libwebkit2gtk-4.1-0, libgtk-3-0`
  plus `libdbus-1-3` (linked directly, credential store) and `ca-certificates`. A fresh
  Debian 12 system without a CA store made the launcher panic at startup because the
  HTTPS client could not load system roots. Startup failures in the setup hook now exit
  with a readable message and code 1 instead of a panic.
- **AppImage:** relies on the host for the libraries linuxdeploy excludes (glibc, ALSA,
  EGL/GL/GLES, GBM, fontconfig, FriBidi, HarfBuzz, Wayland client, X11). Every desktop
  installation has them; the smoke test installs exactly these on a stripped system.
- **Build label:** taken from the Rust core's build profile (`get_app_info`): release
  cores show "Qualification build" (doc 01: packages are private qualification builds
  until P13), debug cores "Development build". Superseded by D-032: release builds show
  no label.
- **Package workflow:** `.github/workflows/package.yml` builds every target, writes
  `SHA256SUMS` and `build-info.json`, smoke-tests (install, start, data root, uninstall
  keeps data) and keeps packages as workflow artifacts only. It never creates a release.
  Apple signing secrets are read only on pushes to `main` and manual runs and only when
  present; Windows signing is an owner decision (docs/PACKAGING.md).
- **Limitation:** CI runners and chroot systems are not consumer machines. Target-machine
  qualification, including Wayland, HiDPI and missing WebView2, is P13.

## D-029 — Qualification findings: free space and webview descriptors

- **Status:** accepted · **Phase:** P13 · **Affects:** A22, A23, MV11
- **Free space:** on Unix the pre-download space check now uses `statvfs` of the data
  directory itself (`rustix`, already in the dependency tree, safe API). sysinfo's disk
  list omits tmpfs and other filesystems, so a data root on one was measured against its
  parent mount and a full disk was only noticed mid-download. Windows keeps the volume
  list, which includes every volume, but no longer canonicalizes the path first: the
  verbatim `\\?\D:\…` form never matched a mount point such as `D:\`, so since P06
  the Windows check silently returned "unknown" and was skipped. The new unit test
  exposed it in CI.
- **Webview descriptors:** destroying and recreating the webview (default "unload and
  return" mode) leaves one `timerfd` and one `WebKitSharedMemory` memfd open per cycle on
  Linux. Child supervision closes its own pipes and pidfds, and "keep open" mode shows no
  growth, so the leak is inside WebKitGTK's teardown. It is documented rather than worked
  around: about 490 cycles fit under a 1,024-descriptor soft limit, and restarting the
  launcher releases them. Re-measure on Windows and macOS, and after WebKitGTK/wry
  updates.
- **Live installation test:** `tests/live_install.rs` drives the real install service for
  any supported version in any data directory (ignored by default), which makes the
  multi-version cache checks (MV01, MV02 install side, MV08) repeatable.

## D-030 — Debug-build test profile for automated launch tests

- **Status:** accepted (owner-approved, 2026-10-17) · **Affects:** A12, A17, A18, A23, MV02
- **Context:** without a project-owned Microsoft client ID no launch can be authorized,
  so nothing after preflight could be tested automatically. The owner approved a test
  profile for automated download and launch tests.
- **Decision** (the profile now lives in `launch::notoken`, see D-033 and D-040): `launch::test_profile` exists only in debug builds and tests
  (`cfg(debug_assertions)`); packages are release builds and contain no code path to
  it. A debug build started with `LAUNCHER_NOTOKEN_PROFILE=<name>` authorizes launches with
  that name, a UUID derived from it and a placeholder token, and always adds Minecraft's
  `--demo` argument (`LaunchIdentity.demo`). The header shows "Test profile <name> ·
  demo mode". `tests/live_play.rs` drives the real game through the launch service with
  the profile.
- **Limits:** demo mode only (5 in-game days, no multiplayer); the placeholder token is
  rejected by every Minecraft service. Evidence gathered with the profile is labelled
  "test profile, demo" and never counts as account verification: A03–A06 and A13 still
  need real accounts, and the Microsoft client ID blocker is unchanged.
- **Found with it:** a game ended through Stop was recorded as "Exited with an error";
  runs now have a `stopped` state ("Stopped by you") and the last result reads
  "Cancelled" instead of "Failed".
- **Graphics on headless Linux:** Minecraft 26.3 needs an sRGB-capable GLX visual that
  Xvfb and the Xorg dummy driver do not offer; with the Mesa software Vulkan driver
  (lavapipe) it uses its Vulkan backend instead. 1.21.1 runs on GLX with llvmpipe.

## D-031 — Neutral application identity

- **Status:** accepted (owner request, 2026-10-17) · **Affects:** FR13, A24, A26; DATA_ROOT,
  credential-vault and single-instance namespaces
- **Decision:** The code and documentation use a neutral identity: product name
  `Launcher`, binary, crate and Debian package `launcher` (library `launcher_lib`),
  bundle identifier `app.launcher`, user agent `Launcher/<version>`. Environment
  variables use the `LAUNCHER_` prefix (`LAUNCHER_MSA_CLIENT_ID`, `LAUNCHER_DATA_ROOT`,
  `LAUNCHER_LIVE_*`); the debug test profile variable is `LAUNCHER_NOTOKEN_PROFILE`
  (D-030), its test player `TestPlayer`. Receipts are `.launcher-tree.json` and
  `.launcher-runtime.json`.
  At the owner's request the README names the project "Sferum Launcher".
- **Reason:** The owner asked for a neutral name. `app.launcher` avoids a repeated
  "launcher.launcher" segment.
- **Consequence:** DATA_ROOT and the vault service move to the new identifier. No package
  has been distributed, so no migration is needed; development data under the old
  identifier is not read. The packaging workflow reads the repository variable
  `LAUNCHER_MSA_CLIENT_ID`. Links to the GitHub repository keep its real address, and
  evidence recorded before this change quotes the names that were in use then.
- **Verification:** `src-tauri/tauri.conf.json`; `npm run check`; release binary scan
  (docs/VERIFICATION.md).

## D-032 — Game-style interface and header mark

- **Status:** accepted (owner request, 2026-10-17) · **Affects:** FR13, A24
- **Decision:** The interface takes a darker, game-like look built only from the design
  tokens: glass panels over a glowing block-grid background, a green-to-cyan accent,
  uppercase tabs and status lights. The selected installation is a hero panel with a
  large Play button over a decorative block landscape drawn as inline SVG from theme
  colors; installations get colored block tiles. The header shows a block logo and the
  mark "Sferum Launcher" ("Sferum" large). Release builds show no build label; debug
  builds keep "Development build" so development windows stay recognizable.
- **Limits:** no external fonts or images (the CSP allows only bundled assets); system
  fonts are used. Light and dark themes both remain; reduced-motion disables
  transitions. The window title, package and binary names stay as in D-031.
- **Verification:** `npm run check`; release build screenshot in the README.

## D-033 — Demo play without an account

- **Status:** accepted (owner decision, 2026-10-17) · **Affects:** A11, A12; supersedes the
  "no demo bypass" rule of docs/spec/README.md and P04 for the demo only
- **Context:** without a project-owned Microsoft client ID nobody can sign in, so a
  packaged build could install the game but never start it. The owner asked for the demo
  to be playable without keys.
- **Decision:** when the account does not have verified access, the Play button becomes
  **Play demo**. It launches Minecraft's demo edition (`--demo`: five in-game days, no
  multiplayer) under the local player name "Player" with a placeholder token that every
  Minecraft service rejects. `launch_game` takes `notoken: true` for it; the Rust core then
  uses `launch::notoken::NoTokenProfile` instead of the account (`LaunchMode::NoToken`), with the
  same installation, integrity and supervision checks as any launch. The debug test
  profile of D-030 is the same profile under another name.
- **Limits:** full play is unchanged: it always goes through the signed-in account and a
  fresh access check, and no build path removes `--demo` from a demo launch. Demo
  evidence never counts as account verification (A03–A06, A13).
- **Verification:** unit tests (`notoken_launches_need_no_account_and_always_run_the_demo`,
  `InstallPanel` demo and Play cases); a release build started the real demo.

## D-034 — Client ID as a repository variable

- **Status:** accepted (owner action, 2026-10-17) · **Affects:** P04, A03
- **Decision:** the project-owned Microsoft client ID
  (`96c77e97-0788-4e3a-8eaf-91c8b143ba86`, a public, non-secret value) is supplied as the
  GitHub repository variable `LAUNCHER_MSA_CLIENT_ID`. The package workflow compiles it
  into every package; local builds pass the same environment variable. It is not
  committed to source, so a fork never ships this project's ID by accident.
- **Registration:** personal Microsoft accounts only (`requestedAccessTokenVersion` 2),
  platform "Mobile and desktop applications", redirect `http://localhost`, no secret.
- **Status of access:** Microsoft's authorization endpoint accepts the launcher's request
  (sign-in form shown); Minecraft service access is pending, so a sign-in is expected to
  end with the Minecraft refusal (`AUTH_FORBIDDEN` / `AUTH_APP_NOT_APPROVED`). **Play demo**
  (D-033) is unaffected.

## D-035 — Every vanilla version: releases, snapshots, beta and alpha

- **Status:** accepted (owner request, 2026-10-17) · **Affects:** FR (version choice),
  MV01–MV12; supersedes the release-only policy of docs/spec/01 ("Supported release
  policy", "Deliberate scope limits": snapshots and legacy argument formats) and the
  curated-list gate of D-021
- **Decision:** the version list is Mojang's catalog: every entry of type `release`,
  `snapshot`, `old_beta` or `old_alpha` (919 versions on 2026-10-17). The catalog's
  descriptor SHA-1 is the trust anchor for every download, as before; the two pinned
  versions keep a **Verified** mark. The picker shows one tab per type with counts,
  a search field, release dates and **Latest**.
- **Older formats supported:**
  - `minecraftArguments` (before 1.13): game arguments split on whitespace, JVM
    arguments supplied by the launcher (natives path, launcher brand, classpath; the
    Intel driver heap-dump flag on Windows);
  - native classifier jars (`natives`, `${arch}` = 64, `extract.exclude`): downloaded
    like libraries, never on the classpath, extracted into the run's natives
    directory with entry names checked;
  - legacy asset layouts: `virtual` (1.6–1.7.2) under `assets/virtual/<index>` and
    `map_to_resources` (before 1.6) under the installation's `resources`, both
    hard-linked (or copied) from the verified objects;
  - placeholders `auth_session` (`token:<token>:<uuid>`, game arguments only, redacted
    like the token), `game_assets`, `user_properties` (`{}`);
  - descriptors without `javaVersion` run on Java 8; Java 16 (Minecraft 1.17), which
    Temurin never shipped, runs on Java 17; Java 8's `1.8` version string is parsed;
  - version IDs with spaces (old pre-releases) are safe names;
  - Mojang's previous hosts `launcher.mojang.com` and `launchermeta.mojang.com`, which
    older descriptors still use, are allowed sources.
- **Limits:**
  - Apple silicon: versions with native classifier jars (everything before 1.19) are
    refused with a clear message: their natives exist only for Intel, and Temurin
    has no Java 8 for Apple silicon.
  - Demo: only clients started through `net.minecraft.client.main.Main` (1.6 and
    later) know `--demo`. A demo identity cannot start older versions
    (`NOTOKEN_UNAVAILABLE`); they need a signed-in account.
  - Mod loaders (`inheritsFrom`) were out of scope here; D-039 adds them.
- **Verification:** every catalog version plans on Windows, Linux and Intel macOS, and
  on Apple silicon from 1.19 (`live_all_versions`); real installs and launches on Linux
  from rd-132211 (2009) to 26.4-snapshot-4 (docs/VERIFICATION.md).

## D-036 — Mojang Java runtimes and one Settings screen

- **Status:** accepted (owner request, 2026-10-17) · **Affects:** docs/spec/04
  ("Managed Java"), D-035 (Java 16 → 17 and Java 8 on Apple silicon), the Settings
  screen
- **Decision:** private Java runtimes come from **Mojang** by default, the runtimes the
  official launcher uses; **Eclipse Temurin** (Adoptium) stays available. The choice is
  `javaProvider` in `settings.json` and applies to new installations; an installed
  version keeps the runtime pinned in its lock, and Repair restores exactly that one.
- **How a Mojang runtime is chosen and installed:**
  - Mojang's runtime index (`piston-meta.mojang.com/v1/products/java-runtime/…/all.json`)
    lists one build per platform (`linux`, `windows-x64`, `windows-arm64`, `mac-os`,
    `mac-os-arm64`) and component. The version descriptor names the component
    (`javaVersion.component`); descriptors without `javaVersion` use `jre-legacy`.
  - When the component is not published for this platform, another component of the
    same Java major is used (Apple silicon has `java-runtime-gamma` instead of
    `-beta`). When none exists — Java 8 on Apple silicon and Windows on ARM, anything
    on Linux ARM — Temurin is used. A failed request is an error, never a silent switch.
  - The component manifest is downloaded at resolve time, checked against the index's
    SHA-1 and size, and pinned by its SHA-256 (`RuntimeKey.archiveSha256`), so a job
    retry or repair installs the same files. Every listed file must be on Mojang's
    download hosts.
  - Install: the manifest by SHA-256, then each listed file by SHA-1 through the
    download executor (identical files once), then the tree is assembled with the
    archive extractor's rules (path checks, case collisions, links only inside the
    runtime, size limits), described by the tree receipt, probed and promoted to
    `runtimes/mojang/<os>-<arch>/<major>/jre/<sha256>`. The downloaded files are
    removed afterwards. Java runs from `bin/java`, `bin/java.exe` or
    `jre.bundle/Contents/Home/bin/java` (macOS).
  - Minecraft 1.17 now gets the Java 16 it declares; Temurin keeps the Java 17
    substitute of D-035.
- **Settings screen:** sections **General** (theme, window while playing, data
  location), **Java** (runtime source and the Java of each installation), **New
  installations** (default memory and window size) and **Installations** (memory and
  window size of an existing installation, moved from the home screen). The home screen
  shows an installation's version, Java, memory and window size, and **Change
  settings** opens its section.
- **Recorded per installation:** `instance.json` gains a display-only `java` field
  (provider, major, release) taken from the lock after an install or repair and at
  startup; the lock stays authoritative.

## D-037 — Console window

- **Status:** accepted (owner request, 2026-10-17) · **Affects:** docs/spec/05
  ("Process supervision"), docs/spec/07 (capabilities), the Diagnostics screen
- **Decision:** a separate **Console** window shows, live, what the launcher does and
  everything the game prints. It opens from **Diagnostics → Open console** or with
  Ctrl+Shift+L (Cmd+Shift+L on macOS), in every build.
- **What is recorded:** a journal kept only in memory (the newest 10,000 entries; a
  line longer than 64 KiB is cut):
  - launcher start (version, OS, data directory, installations, memory limits) and
    storage notices;
  - settings changes, created installations and their memory/window changes;
  - each install or repair phase with file counts, the resolved plan (libraries,
    assets, Java provider, release and component, a Temurin fallback), the result
    with downloaded and reused files, and errors with their code;
  - account state changes (state names and error messages only);
  - each launch: mode, the Java executable, every argument (an option and its value
    on one line) after redaction, run state changes, exit code and duration;
  - every game output line after the same redaction as the run log. Minecraft's
    log4j XML events are folded into one entry each (`[thread/LEVEL] logger:
message` plus a stack trace), with the event's level.
- **Secrets:** game lines and arguments pass through the launch redactor first;
  launcher entries are built from values that never hold a token, an authorization
  code or a password. A test launches a fake game that prints its access token and
  checks that the journal never contains it.
- **Window and permissions:** the console window (label `console`) loads the same
  packaged UI, which renders the console for that label. Its capability grants only
  `read_console`; the main window gains only `open_console`. The window polls
  `read_console(after)` every 300 ms. It stays open while the main window is unloaded
  for a game; closing the main window without a running game closes it too.
- **Not included:** the journal is not written to disk and not part of the
  diagnostics export (the run logs already are).

## D-038 — Interface languages: built for many, English, Russian and Arabic today

- **Status:** accepted (owner request, 2026-10-17) · **Affects:** every screen, the
  console window, core error messages, `settings.json`
- **Decision:** the interface is translatable into any number of languages. It ships
  in **English** (the default), **Russian** and **Arabic**. The choice is
  `language` in `settings.json`, a BCP 47 tag (`en`, `ru`, `ar`, `pt-BR`), picked in
  **Settings → General → Language**; each language is named in itself, with its
  translated share when below 100 %. A change applies at once, without a restart.
- **Catalogs:** `src/locales/messages/<tag>.json`, ICU MessageFormat (plurals,
  selects, number formats), the format translation services read. English is the
  source; `_meta` gives a language's own name, writing direction and optionally its
  numbering system (Arabic uses Latin digits, like version numbers and paths).
  A language is added by adding its file; no code changes. See
  [TRANSLATING.md](TRANSLATING.md).
- **Loading:** English is in the main bundle; every other catalog is a separate file
  loaded when chosen. A missing text comes from the next language in the chain
  `pt-BR` → `pt` → `en`, so a partial translation works.
- **Formatting:** plural forms, numbers and dates through `Intl` and ICU for the
  chosen language; no per-language code.
- **Core messages:** the Rust core sends the English text plus, when it carries
  values, its template and the values (`template`, `params`). Catalogs translate
  templates in their `errors` section; values that are themselves core texts (the
  file operation that failed, what failed to download) are translated too. A message
  a catalog lacks falls back to English; one no catalog knows, to a sentence for its
  error code.
- **Right-to-left:** a catalog with `"dir": "rtl"` sets `dir="rtl"` on the page. The
  CSS uses logical properties (`inline-start`, `text-align: start`); the few visual
  directions (selection marks, hover nudge, the hero scene and its scrim) have
  mirrored rules. Paths and the console journal stay left-to-right.
- **Tooling:** `npm run i18n` copies every user-facing template from the Rust sources
  into the English catalog and regenerates `src/lib/i18n/messages.ts` (typed texts)
  and `src/locales/languages.json` (languages and completeness). `npm run i18n:check`
  (in `npm run check` and CI) fails on invalid ICU, keys English lacks, values a
  source text lacks, a formatted Rust error message, or outdated generated files.
- **Console window:** it cannot read settings, so it follows the language through a
  copy in local storage and retitles itself (`core:window:allow-set-title`).
- **Not translated:** the console journal's own lines, game output, product and
  service names, documentation.
- **Translations:** Russian and Arabic were written for this change and have not yet
  been reviewed by native speakers.

## D-039 — Mod loaders: Forge, NeoForge, Fabric and Quilt

- **Status:** accepted (owner request, 2026-10-17) · **Affects:** docs/spec/04
  ("Metadata resolution": inheritance), docs/spec/05 (`${library_directory}`),
  docs/spec/07 (hosts), D-035 (inheritance was out of scope)
- **Decision:** a new installation can run a mod loader on top of a vanilla
  version. The form offers **Mod loader** (None, Forge, Fabric, Quilt, NeoForge), then
  the Minecraft version (only versions that loader supports are listed), then the
  **loader version** (newest first, the project's recommended build preselected,
  betas marked). Game version and loader version are fixed for the installation's
  lifetime, like the game version alone before.
- **Version lists:** Fabric and Quilt meta (`meta.fabricmc.net/v2`,
  `meta.quiltmc.org/v3`); Forge's `maven-metadata.json` and `promotions_slim.json`
  (files.minecraftforge.net); NeoForge's Maven API. NeoForge versions name their game
  version (`21.1.262` → 1.21.1, `26.3.0.88` → 26.3); its 1.20.1 builds use the old
  `net.neoforged:forge` artifact. Snapshot and April Fools builds are left out. Forge
  builds before a game version's recommended build are marked beta, as Forge itself
  labels them.
- **Supported range:** what each project's metadata lists, except Forge before
  1.12.2 build 14.23.5.2851: those installers use the old format (`install` +
  `versionInfo`) and are neither listed nor installed; a chosen one fails with a clear
  message.
- **How a loader is resolved (`src-tauri/src/loader`):**
  - Fabric and Quilt: the loader's version profile from its meta service. Libraries
    without a published SHA-1 (Fabric's intermediary and loader, every Quilt library)
    are pinned by the `.sha1` file next to them on the project's Maven repository;
    files fetched to learn their size are verified and kept.
  - Forge and NeoForge: the installer jar, pinned by its Maven `.sha1` file. The
    launcher reads its `install_profile.json` (spec 0 or 1) and version profile itself;
    it does not run the installer's own GUI or `main`.
  - The result is stored as a launcher-owned **loader manifest** under its SHA-1
    (`metadata/loaders/<sha1>.json`); the install lock records the loader kind,
    version and that hash. Repair and launch re-derive the exact plan from it without
    the network; if it is missing, the loader is resolved again and must produce the
    same hash, otherwise the installation must be recreated.
- **Plan merge (`install::plan::apply_loader`):** as `inheritsFrom` intends: the
  loader's libraries first, replacing the game's copy of the same library
  (`group:artifact[:classifier]`); the loader's JVM and game arguments after the
  game's (a legacy `minecraftArguments` string, Forge 1.12.2, replaces the game's);
  the loader's main class. Demo mode still requires the _game's_ main class to be
  Minecraft's own; the loaders pass `--demo` on to it, except Forge on Mojang's
  launchwrapper (1.12.2), whose tweaker drops it: demo launches of those are refused
  (`NOTOKEN_UNAVAILABLE`).
- **Maven tree:** loader installations keep their libraries under
  `shared/maven/<Maven path>` with their real file names, because Forge and NeoForge
  build a Java module layer from file names and find libraries and patched game files
  by Maven path (`-p ${library_directory}/…`, `-DlibraryDirectory`). Paths come from
  strictly validated Maven coordinates only. The client jar is named after the loader
  profile (`shared/clients/<sha1>/<profile id>.jar`), which Forge's module loader
  expects on its ignore list. `${library_directory}` is now a known placeholder.
  Vanilla installations keep the content-addressed layout.
- **Forge and NeoForge installer step (`loader::processors`):** after downloads and
  the Java runtime, a new phase _Running the mod loader installer_:
  - files packed under `maven/` in the installer are unpacked after a hash check;
  - each client-side processor runs with the installation's private Java, its tool
    jar and classpath from verified downloads, its arguments from the install
    profile's `data`; declared output hashes are checked, and a processor whose
    outputs already match is skipped;
  - `DOWNLOAD_MOJMAPS` that would copy Mojang's mappings unchanged is replaced by the
    verified `client_mappings` download from the plan. The sanitizing variant (Forge
    1.21+) runs as shipped and downloads the mappings from Mojang itself, through the
    system proxy;
  - tool output goes to the console as a line count, and its last 200 lines when a
    step fails (`LOADER_INSTALL_FAILED`);
  - every file the step produced is hashed into the install lock. Launch checks their
    presence and size like downloaded files; Repair checks their hashes and runs the
    step again when one is damaged, then requires the same hashes.
- **Hosts:** a new source category `ModLoaders`: `meta.fabricmc.net`,
  `maven.fabricmc.net`, `meta.quiltmc.org`, `maven.quiltmc.org`,
  `maven.minecraftforge.net`, `files.minecraftforge.net`, `maven.neoforged.net`. Loader
  profiles also name Mojang's library host; each download uses the category whose
  allowlist names its host.
- **Not included:** installing or updating mods, modpacks, changing the loader of an
  existing installation, Forge before 1.12.2 build 14.23.5.2851, LiteLoader, Rift.

## D-040 — "No token" names in code; Microsoft setup panel removed from About

- **Status:** accepted (owner request, 2026-10-17) · **Affects:** D-030, D-033, D-035,
  D-039 (names only), the About screen
- **Decision:** the code calls the profile that plays without an account the **no-token
  profile** (`launch::notoken::NoTokenProfile`, `LaunchMode::NoToken`,
  `LaunchIdentity::notoken`, the `notoken` field of `launch_game`), matching the
  existing `LAUNCHER_NOTOKEN_PROFILE`. The error code is `NOTOKEN_UNAVAILABLE`; an
  installation record written with the old `DEMO_UNAVAILABLE` still reads.
- **Not renamed:** what belongs to Minecraft, not to the launcher: the `--demo` argument,
  the `is_demo_user` rule feature, and the interface texts ("Play demo"), which name the
  game's demo mode that this profile plays.
- **About:** the "Microsoft sign-in setup" panel is gone. The Account screen still tells
  whether sign-in is available in this build.

## D-041 — Installation notes

- **Status:** accepted (owner request, 2026-10-17) · **Affects:** docs/spec/02
  (`InstanceFile`), the Installations screen
- **Decision:** every installation has free-text notes (`notes` in `instance.json`, up to
  4,000 characters; line breaks and tabs kept, other control characters refused, line
  endings normalized). They are edited under the installation's details and saved with
  **Save notes** (`set_instance_notes`); the list shows their first line. Saving is
  allowed while the installation is installing or playing, because notes affect
  neither.

## D-042 — Close the launcher once the game opens

- **Status:** accepted (owner request, 2026-10-17) · **Affects:** docs/spec/05 ("Window
  and application states"), Settings → While a game is running
- **Decision:** a third choice beside "close the window and reopen it" and "keep it
  open": **Close the launcher once the game window opens; the game keeps running**
  (`duringGameMode: "close_launcher"`). After the run starts, the launcher waits until
  the game's log shows its window (the same markers the live tests use) or 60 seconds
  of running, then quits. A game that ends first keeps the launcher open with the
  result.
- **The game afterwards:** it keeps running on its own; output written after the
  launcher quit is not recorded (the run log ends there). The active-run record stays,
  so a launcher started while that game runs shows it as running, offers **Stop game**,
  and blocks installing and playing until it ends (D-026's survivor handling).
- **Also fixed:** while a game ran, the installation panel showed install progress
  ("Reading version metadata") and a Cancel button; it now says the game is running.

## D-048 — Windows: windows are created outside WebView2 callbacks; Windows 11 on ARM in CI

- **Status:** accepted (owner report and request, 2026-10-17) · **Affects:** D-037
  (console window), the P03 window lifecycle, `.github/workflows/`
- **Problem:** on Windows the console window opened empty. `open_console` is a
  synchronous command, so it runs on the main thread inside WebView2's message
  callback; `run_on_main_thread` called on the main thread runs its task at once, so
  the console's WebView2 was created inside that callback, which WebView2 does not
  support (Tauri documents window creation in synchronous commands and event handlers
  as hanging on Windows). Linux (WebKitGTK) was not affected.
- **Decision:** window creation (the console, and the main window when it is created
  again: after a game, from a second launch or the macOS Dock) is always handed to a
  short-lived thread that queues it on the main thread's event loop, so it never runs
  inside a WebView2 or window-procedure callback.
- **CI:** the Windows package job also runs on **Windows 11 on ARM**
  (`windows-11-arm`, native arm64 build, owner's request), beside `windows-2025` x64,
  and the Rust core tests run there too. The Windows smoke test now opens
  **Diagnostics → Open console** through UI Automation and fails unless the console
  window shows the journal lines the core wrote at start; it uploads a screenshot of
  the window.
