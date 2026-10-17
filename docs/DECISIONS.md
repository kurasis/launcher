# Decision records

Significant implementation decisions and deviations from the specification
([docs/spec/](spec/README.md)). Each entry states the reason, the affected
requirement identifiers and how it is verified. Newest entries are appended at the end.

Status values: `accepted` (in effect), `provisional` (in effect until the owner confirms or
replaces it), `superseded`.

---

## D-001 — Provisional application identity

- **Status:** provisional (owner confirmation required before the first distributed package)
- **Phase:** P01 · **Affects:** FR13, A24, A26; DATA_ROOT, credential-vault and single-instance namespaces
- **Decision:** Product name `Kurasis Launcher`, bundle identifier `io.github.kurasis.launcher`,
  binary `kurasis-launcher`, derived from the repository owner. The name contains no
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
- **Decision:** The Rust core reads `KURASIS_MSA_CLIENT_ID` at compile time
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
- **Relocation:** debug builds honour `KURASIS_DATA_ROOT` for tests and demonstrations.
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
  `--kurasis-lifecycle-test-child <scenario>`; scenarios are a fixed enum (short, long,
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
- **Trust:** a version is offered only when it is on the curated compatibility list and the
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
- **Tree receipt:** `.kurasis-tree.json` lists every directory, file (size, SHA-256,
  executable bit) and link target, sorted by path; its SHA-256 is stored in
  `.kurasis-runtime.json` together with the pinned package and the probe result, and goes
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
  stored in `locks/active-run.json`. Only an exact match is treated as the game; it is
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
- **Build label:** packaged builds show "Qualification build" (doc 01: packages are
  private qualification builds until P13); development builds keep "Development build".
- **Package workflow:** `.github/workflows/package.yml` builds every target, writes
  `SHA256SUMS` and `build-info.json`, smoke-tests (install, start, data root, uninstall
  keeps data) and keeps packages as workflow artifacts only. It never creates a release.
  Apple signing secrets are read only on pushes to `main` and manual runs and only when
  present; Windows signing is an owner decision (docs/PACKAGING.md).
- **Limitation:** CI runners and chroot systems are not consumer machines. Target-machine
  qualification, including Wayland, HiDPI and missing WebView2, is P13.
