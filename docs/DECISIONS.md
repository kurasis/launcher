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
  `Object.prototype` is frozen in the launcher's own pages by `FREEZE_PROTOTYPE_SCRIPT`
  (`src-tauri/src/lib.rs`). Tauri's `freezePrototype` is off: WebView2 ran it in every
  frame, cross-origin ones included, and the player window's YouTube and Twitch players
  stayed black (D-067).
- **Verification:** `security_config.rs` rejects remote sources, `unsafe-eval` and
  wildcards; release build renders correctly under the production CSP.

## D-007 — CI matrix and scope

- **Status:** accepted · **Phase:** P01 · **Affects:** FR13, A25
- **Decision:** `.github/workflows/ci.yml` runs frontend checks on `ubuntu-24.04` and, on
  `ubuntu-24.04`, `windows-2025`, `macos-15` (arm64) and `macos-15-intel` (x64): clippy with
  `-D warnings` and `cargo test --locked`. Rustfmt runs once on Linux. The workflow has read-only
  permissions and uses no secrets. Installer packaging and artifact upload belong to P12.
- **Limitation:** A CI build is compile evidence, not a target-machine test.
- **Amended 2026-10-20:** the release build without installers (`tauri build --no-bundle`)
  left CI. The Package workflow builds the same release on every target and smoke-tests it,
  so the CI copy only doubled the slowest step (15–19 min on Windows) and held runners.
  macOS (CI and Package) runs only on a manual run with the `macos` input, until the owner
  turns it back on; the release archive needs it, so it is built from such a run.
  Windows on ARM (CI and Package) likewise runs only on a manual run with the
  `windows_arm` input, until the owner turns it back on; the release archive comes from a
  run with both inputs.
- **Amended 2026-10-22 (owner request):** the live world servers (9 vanilla releases and
  the `on_every_change` releases with mods, about 30 jobs) run on a pull request only when
  it changes `src-tauri/`, `rust-toolchain.toml`, `ci.yml` or `world-mods.yml`; pushes to
  `main`, manual runs and the nightly run try them always. The check
  `World server (linux-x64)` passes without them when the launcher did not change. The
  live test is built once per run (`Build the live test`) and every server job, with and
  without mods, runs that binary instead of compiling it again. With several pull requests
  at once, those jobs had kept other pull requests waiting about 30 minutes for runners.
  **Amended again 2026-10-22 (owner request):** a pull request runs them only when it
  changes the worlds' code (`src-tauri/src/worlds/`, `src-tauri/src/tunnel/`), their live
  test (`src-tauri/tests/live_server.rs`) or `world-mods.yml`. Changes to code they share
  with the rest of the launcher (downloads, loaders, mods, Java) are tried by the push to
  `main` and the nightly run.
- **Amended 2026-10-22 (owner request):** every Ubuntu package install goes through
  `.github/actions/apt-install` (the Package workflow's Debian container repeats its loop
  inline): each `apt-get update` and `install` has a time limit, and a try that runs out
  drops the runner's Azure mirror and tries again from `archive.ubuntu.com`, three tries in
  all. The step stops at 25 minutes and every job has its own limit. A job had waited two
  hours on an Azure mirror that stopped answering.

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
- **Amendment (2026-10-22, Player test):** versions started by Mojang's launchwrapper
  1.5 with its default tweaker (every release before 1.6, beta, alpha) get the
  launcher's own tweaker, `app.launcher.legacy.LegacyTweaker` (`java/legacy-tweaker`,
  about 30 lines, built for Java 8 into a committed jar that CI rebuilds and checks).
  Mojang's tweaker stores the game folder in the game's static `File` field only at
  the end of `Minecraft.main`, after the game thread has started; when that thread
  reads the field first, the game takes the system user's default `.minecraft` for its
  options, saves and statistics (seen with 1.2.1–1.2.4 on Windows). The launcher's
  tweaker is Mojang's with the same field set once the class is loaded, before `main`;
  anything unexpected leaves Mojang's behaviour. The jar is written to
  `shared/launcher/legacy-tweaker-<sha256 prefix>.jar`, added to the classpath and named
  with `--tweakClass`. A descriptor that names its own tweaker keeps it.
- **Amendment (2026-10-23, special versions on Windows):** the oldest games, Classic
  to Alpha 1.0 (Mojang's `AlphaVanillaTweaker`, launchwrapper 1.5 and 1.6; 7 Mojang
  versions and 79 of the special ones, D-073), run in launchwrapper's own frame, which
  ends the game with code 1 and without a word when the player closes the window, the
  code a failed start ends with too: the launcher showed every such close as a failure.
  The same jar has `app.launcher.legacy.AlphaTweaker`, Mojang's tweaker with a toolkit
  listener (it hears the window's close before the frame's own listener) that prints
  `Launcher: the game window was closed`; it takes the place of `AlphaVanillaTweaker`
  in the arguments, and code 1 after that line within the output's last ten lines is
  a normal end, as after "Someone is closing me!". CI starts a stand-in game in both
  launchwrapper versions under Xvfb, closes its window and checks code and line.
  Classic's exit hung on Windows after that line (the Player test and the owner's own
  click): launchwrapper's shutdown hook pauses the applet, Classic's paused loop only
  sleeps, and its thread no longer handles the messages of the windows it owns that
  the input and display hooks wait on. The tweaker halts the game with code 1 when it
  still runs 3 s after the close, and says so; CI checks it with a stand-in whose
  shutdown hook never ends.

## D-036 — Mojang Java runtimes and one Settings screen

- **Status:** accepted (owner request, 2026-10-17) · **Affects:** docs/spec/04
  ("Managed Java"), D-035 (Java 16 → 17 and Java 8 on Apple silicon), the Settings
  screen
- **Decision:** private Java runtimes come from **Mojang** by default, the runtimes the
  official launcher uses; **Eclipse Temurin** (Adoptium) stays available. The choice is
  `javaProvider` in `settings.json` and applies to new installations; an installed
  version keeps the runtime pinned in its lock, and Repair restores exactly that one
  (until the user switches it, D-044).
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
- **Not included (then):** the journal was not written to disk nor exported. Since
  D-061 it is also kept as `logs/launcher.log` and is part of the diagnostics export.

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
  lifetime, like the game version alone before. (The loader version can now be changed, see D-054.)
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
  message. Since 2026-10-21 (skins mod for old versions) the last Forge builds for
  1.7.10 (10.13.4.1614) and 1.8.9 (11.15.1.2318), and since 2026-10-23 those for 1.9.4
  (12.17.0.2317), 1.10.2 (12.18.3.2511) and 1.11.2 (13.20.1.2588), whose installers have
  the same form, are the exception: the old profile's
  libraries are rewritten to the current form (the universal jar from the installer,
  the rest from Forge's Maven or Mojang's library host, pinned by `.sha1` or the
  declared checksum, server-only ones left out) and nothing runs at install time.
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
  - when that Java is a Java 8 older than 8u202 (Mojang's `jre-legacy` for Windows is
    8u51), the step runs on Temurin 8 instead, installed like any private runtime (fix
    of 2026-10-22): such a Java writes zip entries dated before 1980 without the
    extended timestamp, so the outputs of Forge 1.13–1.16 never match the declared
    hashes. The game itself still runs on the chosen Java. The same holds for a world's
    server;
  - before a processor runs, a declared output that does not match and the tool's
    `.cache` file next to it are removed: jarsplitter 1.1.0 (Forge 1.13.2) trusts its
    cache and would keep an output written wrongly by an earlier run;
  - `DEOBF_REALMS` (Forge 1.14.3) looks for the version JSON next to the game jar and
    for libraries three folders up, as in a vanilla game folder; the launcher passes
    `--json` (the game's Realms library only) and `--libs` (the Maven tree) instead;
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
  existing installation, Forge before 1.12.2 build 14.23.5.2851 (except the last 1.7.10,
  1.8.9, 1.9.4, 1.10.2 and 1.11.2 builds), LiteLoader, Rift.
- **OptiFine on its own** (owner request, 2026-10-21): a fifth choice, **OptiFine**,
  without Forge (`loader::optifine`). Builds come from the mods catalog (D-066, project
  `optifine`), not from optifine.net: only versions the catalog serves (kept, not held
  back) for exactly one game version are listed, previews marked beta, the newest
  release recommended. The id is the file name without `OptiFine_` and `preview_`
  (`1.21.1_HD_U_J1`). Resolving downloads the jar from `files-mods`/`cdn-mods`
  (`ModsCatalog`, our server as R2's reserve), checks the catalog's SHA-256 and keeps it
  as `optifine:OptiFine:<id>:installer`. The jar is its own installer, and the launcher
  does what that installer does for a vanilla game: `optifine.Patcher` (named in the
  manifest, since the jar's `Main-Class` is the installer window) builds
  `optifine:OptiFine:<id>` from the client jar with the installation's Java;
  `launchwrapper-of-<v>.jar` at the jar's root (`<v>` from `launchwrapper-of.txt`) is
  unpacked as `optifine:launchwrapper-of:<v>`; the game starts as
  `net.minecraft.launchwrapper.Launch` with `--tweakClass optifine.OptiFineTweaker`
  after the game's arguments. The patcher's output differs from run to run, so it has
  no hash in advance; its hash is recorded once it is built, like Forge's. Builds
  without `launchwrapper-of` (1.7.10) are refused as an older installer format.
  OptiFine takes no mods (the Mods tab says so; shaders and resource packs go in) and no
  worlds yet. The skins mod comes as its Java agent (D-065), from mod 0.8.0 on: an older
  pinned or listed mod gives none, since its agent cannot reach launchwrapper's class
  loader.

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
- **Release builds:** the type name `NoTokenProfile` now appears in release binaries (it
  is the shared profile's name); the debug-only switch `LAUNCHER_NOTOKEN_PROFILE`
  stays absent (`strings` check, 2026-10-17).
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

## D-043 — Choosing the data folder; portable mode

- **Status:** accepted (owner request, 2026-10-17) · **Affects:** docs/spec/02 ("Data
  directories"), Settings → General
- **Decision:** the data folder is chosen at start, in this order:
  1. **Portable mode:** a `portable.txt` file next to the launcher (next to the `.app`
     on macOS, next to the AppImage on Linux) keeps all data in `launcher-data` beside
     it, so the whole folder can move to another computer.
  2. **Chosen folder:** `data-location.json` in the per-user configuration directory
     (separate from the data) names a folder picked in Settings.
  3. **Default:** the per-user application data directory, as before.
- **Moving:** Settings → General → _Launcher data location_ shows the folder and how it
  was chosen, and offers _Move to another folder…_ (native folder picker), _Move back to
  the default folder_ and _Make portable_. After a confirmation the launcher copies its
  data while nothing else may run (a new activity kind, `relocate`), skipping the
  instance lock and download staging, then records the choice (or writes/removes
  `portable.txt`). The new folder is used from the next start; the old folder is kept
  for the user to delete. A target that already holds launcher data (for example the
  folder left behind by an earlier move) is used as it is, without copying. Otherwise the
  target must be empty or missing. It must not contain or lie
  inside the current folder, and must pass the usual data-root checks; symbolic links
  stop the copy, and a failed copy is removed.
- **Portable mode and sign-in:** Microsoft sign-in secrets stay in the operating
  system's credential store, never in the portable folder; on another computer the
  account has to sign in again. The system web view keeps its own cache in the user
  profile.
- **Debug builds:** `LAUNCHER_DATA_ROOT` still overrides everything for tests.

## D-044 — Changing an installation's Java; own Java and JVM options

- **Status:** accepted (owner request, 2026-10-17) · **Affects:** D-036 ("an installed
  version keeps the runtime pinned in its lock"), docs/spec/04 ("Managed Java",
  "Repair"), docs/spec/10 (runtime lock), Settings → Installations
- **Decision:** Settings → Installations gets _Java of this installation_ with three
  choices and a JVM options field:
  - **Private runtime from Mojang** or **from Eclipse Temurin.** Choosing the other
    source for an installed version records it (`javaProvider` in the instance) and
    offers **Repair now**. That repair, and only a repair the user starts, pins a new
    runtime for the version's Java requirement from that source (Temurin when Mojang
    publishes none for this computer, with a console warning), downloads and verifies
    it like any runtime, and then rewrites the `runtime` section of the install lock.
    Every other part of the lock (descriptor, artifacts, loader files) stays as it was;
    a repair without a pending choice restores exactly the locked runtime, as before.
    Until the repair succeeds, the game runs on the runtime in the lock. Before the
    first install, the choice decides the source of the first runtime.
  - **My own Java:** a path to a Java program (typed or chosen with the file dialog),
    available once the version is installed. The launcher starts it once with the
    sanitized environment and reads its version, vendor and architecture. It compares
    them with the Java the game was built for (the lock's `javaMajor`) and the
    computer's architecture. An older Java ("will most likely not start"), a newer one
    ("recent versions usually run; older versions and mod loaders often fail") and
    another architecture are **warnings**: the user can still choose **Use anyway**,
    and the accepted warnings are stored with the choice. A program that does not
    start or does not report Java settings is refused. At every launch the chosen
    Java is checked to start again; if it no longer does, the launch fails with a
    clear message and nothing is marked broken. The private runtime stays installed
    (mod loader installers still use it during Repair).
  - **JVM options:** one line, split on spaces, with quotes keeping spaces inside an
    option. Each option must start with `-` (a bare word would be taken for the main
    class; values go after `=` or `:`), at most 64 options and 4000 characters. They
    are added after the version's own JVM options, so for a repeated option such as
    `-Xmx` the user's value wins; the launcher's token never goes into JVM options.
- **Why the lock may change:** the user asked to change Java after creating a version.
  The lock still pins exactly one verified runtime at all times; only an explicit
  repair replaces it, under the global activity permit, and the commit point is the
  new lock written after the new runtime is verified.

## D-045 — Several accounts, no-token profiles and who plays each installation

- **Status:** accepted (owner request, 2026-10-17) · **Affects:** docs/spec/03 (one
  account, sign-out transaction), D-033 (demo play), the Account screen (now
  **Accounts**), `accounts.json`
- **Decision:**
  - **Microsoft accounts:** any number. _Add a Microsoft account_ runs the same browser
    sign-in as before; a sign-in for an account that is already saved renews it. Each
    account has its own session, refresh credential (vault key
    `msa-refresh:<profile ID>`), state, error and help links, **Check again** and
    **Remove**. Removing one
    account deletes only its credential; a deletion that fails is kept as a pending
    sign-out per account and retried at start, never restored. Every launch
    re-verifies the account it plays as, as before.
  - **No-token profiles:** named local players for Minecraft's demo (3–16 letters,
    digits or `_`, unique ignoring case), added and removed on the Accounts screen.
    They play only the demo, exactly like D-033's built-in player, whose name stays
    `Player`.
  - **Who plays:** one **default** profile (the first account or profile added
    becomes it; any other can be chosen with _Use by default_), and a **Play as**
    choice under each installation's Play button: the default, an account or a
    profile. The launch resolves the installation's profile, else the default, else
    the built-in demo player; a removed profile falls back the same way. A Microsoft
    account plays the full game after its fresh access check; a profile plays the
    demo. The `launch_game` request carries only the installation.
  - **Storage:** `accounts.json` keeps `accounts`, `notokenProfiles`,
    `defaultProfile` and `pendingSignOuts`; an installation keeps `profile`. The single
    `active` account (and `signOutPending`) of earlier versions is moved into the list,
    made the default, and saved in the new form at start.
- **Concurrency:** a sign-in result commits only while the sign-in generation is
  current (cancel advances it); a refresh commits only while its account's generation
  is current (that account's removal advances it), so removing one account never
  disturbs another and a late refresh cannot bring a removed account back.
  `accounts.json` is changed in one step under the state lock by both the auth service
  and the profile commands.
- **Debug builds:** `LAUNCHER_NOTOKEN_PROFILE` still makes every launch a demo as that
  test profile (D-030).

## D-046 — One Play button; no demo hint

- **Status:** accepted (owner request, 2026-10-17) · **Affects:** D-033, D-045, the
  installation panel
- **Decision:** the button reads **Play** for every player, a Microsoft account or a
  no-token profile; the separate "Play demo" label and the hint under it ("Without a
  signed-in account you can play the Minecraft demo…") are removed in every language.
  What a launch plays stays visible where the player is chosen: _Play as_ names demo
  profiles "Name (demo)", and the Accounts screen explains no-token profiles. The
  launch itself is unchanged: a no-token profile always runs with `--demo`.

## D-047 — Incremental backups of worlds and installations

- **Status:** accepted (owner request, 2026-10-17) · **Affects:** a new **Backups**
  screen, `settings.json`, `DATA_ROOT/backups`, the activity permit (new kinds
  `backup`, `restore`)
- **What is backed up:** one world (`saves/<name>` of an installation's game folder)
  or the whole game folder (worlds, mods, settings, resource and shader packs). The
  game, libraries and Java that Repair downloads again are not included.
- **Incremental store:** every file is stored once under `backups/objects/` by its
  SHA-256, and each backup is a manifest in `backups/snapshots/<id>.json` listing its
  files (path, size, modification time, SHA-256). A file whose size and modification
  time match the previous backup of the same world or installation is not read again.
  Files that did not change, or that another backup already holds, cost nothing: a
  backup shows its total size and the bytes it added. Symbolic links are skipped.
- **Restore:** in place, or (for a world) as a copy named "<world> (backup <date>)".
  A restore writes into a new folder beside the target, checks every file against its
  SHA-256, and only then swaps it in, so a failed or damaged restore keeps the current
  folder. Before an in-place restore the current state is backed up ("Before a
  restore").
- **Automatic backups:** _Back up changed worlds after playing_ (off by default) backs
  up, when a game session ends, each world whose files changed since its last backup,
  and keeps the newest _N_ automatic backups per world (default 10, 1–100). Backups the
  user makes are kept until deleted.
- **Management:** the Backups screen lists each installation's worlds with their last
  backup, and every backup with its reason, date, version and sizes. Restore, restore
  as a copy and delete ask for confirmation where they replace or remove data. Deleting
  removes the files no other backup uses, and the screen shows the space all backups
  take.
- **Safety:** backups and restores hold the activity permit, so they never run while
  the installation's game, an install or another backup runs. Manifest paths are
  checked so a manifest cannot write outside the target folder.

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

## D-049 — Download manager and a queue for installs and repairs

- **Status:** accepted (owner request, 2026-10-17) · **Affects:** the installation
  panel, `install_instance`, `repair_instance`, `cancel_operation`, the app snapshot
  (`queue`) and progress events (`currentFiles`)
- **Queue:** an install or repair asked for while another operation runs waits in a
  first-in, first-out queue instead of being refused, and starts by itself when the
  launcher is free (checked every 500 ms). One entry per installation: asking again
  for a queued or running installation is refused as before. `cancel_operation`
  with a queue entry's ID removes it. The queue lives in memory only: closing the
  launcher forgets it, and nothing half-done is left behind because a queued request
  has not started.
- **Downloads row:** in concept A (D-072) Downloads is a row of the rail. It shows the
  running install or repair's percent and how many requests wait. Clicking it opens a
  panel beside the rail, above the launch bar, with the running operation
  (installation, install or repair, phase, percent, downloaded and total bytes, speed,
  files, Cancel), the queue (each entry can be removed) and the last three results
  with when they ended and what they downloaded; the version shows where installations
  share a name. On a metered connection (D-051) it also shows the month's traffic.
- **Launch bar:** while an install or repair runs, the launch bar shows it (name,
  phase, bytes, files, a bar and Cancel) in place of the chosen installation, unless
  that one can still be added to the queue.
- **Percent:** bytes while downloading; checked files while the launcher checks what
  is already present, before any byte is known.
- **File names:** progress names the files being transferred at that moment, at most
  as many as there are parallel downloads. Libraries and the client show their file
  name; game assets, stored by hash, show their logical name (`step5.ogg`); Mojang's
  Java files show their own name. The panel shows one name at a time and moves to the
  next every 1.2 s, so a name stays readable however often progress arrives. Names are
  display only and never paths.

## D-050 — Screenshot gallery across installations

- **Status:** accepted (owner request, 2026-10-17) · **Affects:** a new **Screenshots**
  screen, the `gallery:` protocol, the content security policy (`img-src`),
  `DATA_ROOT/cache/thumbnails`
- **What it shows:** the PNG files Minecraft saves in `screenshots/` of each
  installation's game folder. They are grouped by installation, each group headed by
  the installation's name, its game and loader version ("Minecraft 1.20.1 · Forge
  47.4.10") and the count, so screenshots of installations with the same name stay
  apart. The installation with the newest screenshot comes first, and each group is
  newest first. A filter shows one installation.
- **Viewer:** a screenshot opens at full size with its installation, version, file
  name, date, size and pixel size. Previous and next (buttons or arrow keys) move
  through all shown screenshots across groups; Escape closes. From the viewer a
  screenshot can be opened in the system image viewer, shown in the file manager, or
  deleted after confirmation.
- **How images reach the page:** the webview never gets file paths or a general
  file permission. Rust registers a `gallery:` protocol
  (`gallery://localhost/…`, `http://gallery.localhost/…` on Windows) that answers
  only `full/<installation ID>/<file>` and `thumb/<installation ID>/<file>` for a
  known installation, a plain `.png` name in its `screenshots/` folder and a regular
  file (not a link). The policy's `img-src` allows this protocol and nothing else
  new.
- **Thumbnails:** made in Rust on first view (decoded with a 256 MiB limit, averaged
  down to 480 pixels wide, never scaled up) and kept in
  `DATA_ROOT/cache/thumbnails`, named by the screenshot and its size and time so a
  changed file gets a new one. Listing the gallery deletes thumbnails of screenshots
  that are gone; the cache can be deleted at any time.
- **Commands:** `list_screenshots`, `delete_screenshot`, and `open_screenshot`
  (opens it or, with `reveal`, shows it in the file manager), each taking an
  installation ID and a file name that Rust checks.

## D-051 — Metered connections: asking before big downloads, traffic count and a monthly limit

- **Status:** accepted (owner request, 2026-10-17) · **Affects:** Settings → **Network**,
  `settings.json` (`metered`, `meteredWarnMib`, `monthlyLimitMib`),
  `DATA_ROOT/traffic.json`, installs and repairs (a new phase `awaiting_approval`),
  the download manager
- **Switch:** _Metered connection_ is off by default and set by the user. The
  launcher does not read the system's metered flag: each platform reports it
  differently, and a wrong guess would either nag or download over a capped plan
  without asking.
- **Asking first:** on a metered connection, once an install or repair knows which
  files are missing, a download of at least _N_ MiB (default 100; 0 asks before every
  download) or one that would take this month's traffic past the monthly limit
  waits for the user. A dialog opens by itself (concept A, D-072) with the size, the
  file count and the installation, or the limit and what was used, with **Download**
  and **Don't download**; the dialog holds the focus, so a stray Enter answers
  nothing. Closed without an answer, the operation keeps waiting: the Downloads button
  stays highlighted, and the installation panel and the Downloads panel offer **Show
  the question**. Declining cancels the
  operation like **Cancel**: nothing is downloaded and verified files are kept.
  Once allowed, the rest of that operation (for example its Java) does not ask again.
  The check happens before any byte of the files is fetched, after the free disk space
  check.
- **Traffic count:** every response body the launcher reads is counted where it is
  read, by source: game files and Mojang's Java, Java from Adoptium, mod loaders, and
  version lists. Counts are written to `traffic.json` every 5 seconds and on exit,
  under the current month in local time; twelve months are kept. The Network section
  shows this month by source, the limit as a bar, earlier months, and can reset the
  count. Sign-in requests and the game's own traffic (multiplayer, skins, Realms) are
  not counted.
- **Monthly limit:** 0 (the default) is no limit. The limit never blocks a download
  silently; passing it always needs the user's answer.

## D-052 — Offline: the launcher and installed games work without internet; it checks again when the connection returns

- **Status:** accepted (owner request, 2026-10-17) · **Affects:** a header status, the
  installation panel, the download queue, account checks before a launch, the
  version list
- **What works offline:** the launcher starts and shows everything it has; installed
  versions start, as a launch reads the version, libraries, assets and Java from the
  install record and the shared store and downloads nothing; no-token profiles play as
  always. The version list comes from its cached copy.
- **Microsoft accounts offline:** when the sign-in or Minecraft services cannot be
  reached at all (a network error, not an answer), a saved account starts without the
  online check. It was checked for a Minecraft licence when it was signed in, as the
  official launcher allows offline play after a sign-in. The game gets the last
  Minecraft token of this run (it may have expired) or the placeholder `0`;
  single-player works, multiplayer and Realms need the connection. An answer from the
  services (no licence, a revoked sign-in, an outage reported as 5xx) is never
  overridden. Accounts that were never signed in cannot play offline.
- **Knowing it is offline:** a probe asks a Mojang metadata host for headers only,
  through the usual allowlisted client: at start, every 30 seconds while offline,
  every 5 minutes while online, after each install or repair ends, and when the user
  presses **Check again** next to the **Offline** status in the header.
- **Downloads offline:** installs and repairs asked for while offline wait in the
  queue ("Waiting for the internet connection") and start by themselves when the
  connection returns; the installation panel explains what works offline.
- **When the connection returns:** saved accounts are refreshed and checked again,
  queued downloads start, and an open new-installation form fetches the version list
  again.

## D-053 — Smaller interface changes: the default player once, no "(demo)", Accounts order, while-playing order

- **Status:** accepted (owner request, 2026-10-18) · **Affects:** _Play as_, every
  label of a no-token profile, the Accounts screen, Settings → General
- _Play as_ lists the default player once, as "Default: <name>"; the same profile is
  not repeated below it. An installation assigned to the profile that is the default
  shows as the default.
- No-token profiles are shown by their name alone, without "(demo)", in every
  language and everywhere (_Play as_, "Plays as" in the header). Microsoft accounts
  keep "(Microsoft)", which tells a signed-in account from a profile with the same
  name. What a no-token profile plays is explained on the Accounts screen.
- The Accounts screen shows **Profiles without an account** first, then **Microsoft
  accounts**.
- _While a game is running_ lists: close the launcher window and reopen it after the
  game; close the launcher once the game window opens; keep the launcher window open.

## D-054 — Changing the mod loader version after install

- **Status:** accepted (owner request, 2026-10-18) · **Affects:** Settings →
  Installations, `set_instance_loader`, D-039
- An installation with a mod loader shows **Mod loader version** in Settings →
  Installations: the current version, then the project's versions for this
  Minecraft version (newest first, recommended and beta builds marked). The installed
  version stays listed even if the project withdrew it.
- Choosing another version warns that mods made for the old version may not work and
  suggests a backup of the worlds first; **Change and install** saves the version and
  starts an install at once (or leaves it to an install already waiting in the
  queue, which reads the installation when it starts).
- Only the version changes; the loader itself (Forge, Fabric, Quilt, NeoForge) and
  the Minecraft version stay. Switching loader would invalidate every mod, so it
  remains a new installation.
- The core validates the version id as at creation, refuses while the installation
  is busy, and marks the installation _not installed_: it cannot start with the old
  loader's record. The install sees that the record names another loader and
  resolves the new one; the game files are reused, and mods, worlds, settings and
  screenshots are not touched. Libraries of the previous loader stay in the shared
  library folder and are no longer on the class path.

## D-055 — Deleting installations, with a backup of what the player made

- **Status:** accepted (owner request, 2026-10-18) · **Affects:** installation details,
  Backups, `inspect_instance`, `delete_instance`, `restore_backup`
- **Delete installation** sits in an installation's actions menu and asks in a dialog
  (concept A, D-072), with Cancel holding the focus. It first asks the core what
  the game folder holds: worlds (by name), mods, resource packs, shader packs and
  screenshots, and the folder's size. With any of them it offers **Back up and
  delete** (a backup of the whole installation, reason "Before deleting"), **Delete
  without a backup** and **Cancel**; without them a plain delete and cancel. Settings
  and logs alone do not count as player data.
- Deleting is refused while anything runs (the activity permit, as for backups). The
  installation's folder is first moved aside with one rename and then removed, so an
  interrupted delete never leaves half an installation; leftovers are removed at the
  next start. Its queued installs and repairs are dropped and the oldest remaining
  installation is selected. Game files shared with other installations (libraries,
  assets, Java runtimes) stay.
- Backups now record the Minecraft version and loader. Backups of deleted
  installations are listed under **Deleted installations** on the Backups screen. A
  backup of a whole installation **brings it back**: the same id and name, the game
  version and loader from the backup, default memory and window, and its game folder
  restored; it is then not installed until the user installs it. World backups of a
  deleted installation (and whole-installation backups made before this change) can
  only be deleted.
- A new `instance_removed` event tells the interface to drop the installation.

## D-056 — Minecraft arguments per installation and fullscreen

- **Status:** accepted (owner request, 2026-10-18) · **Affects:** Settings →
  Installations, Settings → New installations, launch arguments
- Settings → Installations has **Start the game in fullscreen** and **Minecraft
  arguments**, saved with memory and window size. Settings → New installations has
  the same fullscreen box; new installations take it.
- Fullscreen passes `--fullscreen` to the game; the window size still applies when
  the player leaves fullscreen (F11).
- Minecraft arguments are typed as one line and split like JVM options (quotes keep
  spaces inside one value, at most 64 arguments and 4000 characters, no control
  characters). They come after the version's own arguments and are passed as typed,
  without `${...}` substitution. Options the launcher sets itself are refused with a
  message naming them: the player's identity and token (`--username`, `--uuid`,
  `--accessToken`, `--clientId`, `--xuid`, `--userType`, `--userProperties`,
  `--profileProperties`, `--session`), `--version`, `--versionType`, the folders
  (`--gameDir`, `--assetsDir`, `--assetIndex`), `--demo`, the window (`--width`,
  `--height`, `--fullscreen`) and the loaders' own (`--launchTarget`, `--fml.*`). A
  second copy would either make the game's option parser fail or let a typed value
  replace the signed-in player.
- The core checks the arguments when they are saved and again at launch.

## D-057 — Settings → Reset

- **Status:** accepted (owner request, 2026-10-18) · **Affects:** Settings,
  `reset_preview`, `reset_launcher`
- A **Reset** section in Settings lists the parts that can be put back to their
  first-start state, each with what it holds now: **Installations** (count),
  **Downloaded game files** (libraries, assets, Java runtimes and version metadata,
  with their size), **Backups** (count and size), **Accounts and profiles** (Microsoft
  accounts and profiles without an account), **Launcher settings**, **Traffic
  statistics** and **Temporary files** (unfinished downloads, thumbnails, game logs,
  with their size). Nothing is chosen at first.
- When installations are chosen, the ones holding worlds, mods, packs or screenshots
  are listed with what they hold, and **Back them up before deleting** is on by
  default: each is backed up as a whole installation and can be brought back from
  Backups (D-055).
- **Reset…** shows the chosen parts and "This cannot be undone." before **Reset**.
- The core refuses while anything runs, then works in a fixed order: existing backups
  are deleted first (so the new backups of installations are kept even when
  **Backups** is chosen), then installations (backup, then delete), downloaded game
  files (installed versions are marked as needing a repair, which downloads them
  again), temporary files (the launcher's own log stays), traffic statistics,
  profiles without an account, settings; Microsoft accounts are signed out last
  through the auth service, as with **Sign out**.
- Launcher settings return to their defaults except the language and the selected
  installation; the data folder location (portable or not) is a separate setting and
  stays.

## D-058 — Memory recommended by the kind of installation

- **Status:** accepted (owner request, 2026-10-18) · **Affects:** new installations,
  Settings → Installations, Settings → New installations, `recommend_memory`
- **Measured** (release launcher, Linux, Minecraft 1.20.1 joining the same world,
  `-Xmx8192M`; heap from `jstat`, live set after a forced full GC; software
  rendering, so load times are long):

  | Installation                  | Mods | Live after GC | Old-gen peak | Heap peak | Process RSS |
  | ----------------------------- | ---- | ------------- | ------------ | --------- | ----------- |
  | Vanilla                       | 0    | 356 MiB       | 862 MiB      | 1623 MiB  | 2.0 GiB     |
  | Fabric 0.19.5                 | 0    | 380 MiB       | 866 MiB      | 1492 MiB  | 2.2 GiB     |
  | Forge 47.4.10                 | 0    | 428 MiB       | 910 MiB      | 2178 MiB  | 2.4 GiB     |
  | Fabric + Fabulously Optimized | 48   | 328 MiB       | 382 MiB      | 982 MiB   | 2.4 GiB     |
  | Fabric + Prominence II        | 442  | 4299 MiB      | 3060 MiB     | 5125 MiB  | 10.7 GiB    |

  A loader adds little by itself, and a performance pack even lowers the heap; what
  needs memory is many content mods (about 9 MiB live per mod in Prominence II).
  Outside the heap the game uses 1–3 GiB more (native memory, graphics), so the
  heap never gets more than half of the computer's memory. Better MC 4 (Forge, 330
  mods) could not be measured: several of its mods are only on CurseForge and the
  pack stops at a "missing mods" window.

- **Recommendation:** Minecraft 1.12.2 and older (and alpha, beta, classic) without
  a loader 2048 MiB; newer Minecraft without a loader 3072 MiB; with a loader 4096
  MiB up to 50 mods, 6144 MiB up to 150 mods, 8192 MiB above. Never more than half
  of the physical memory (in 256 MiB steps) nor the machine's allowed maximum; never
  below 1024 MiB. When the computer cannot give the full amount, the hint says what
  the installation would use.
- **New installations** get the recommendation for their version and loader (no mods
  yet) unless Settings → New installations fixes one amount for all ("The same for
  every new installation"); "Recommended for each kind of installation" is the
  default and goes back to it. The new-installation form says the memory it gets.
- **Settings → Installations** shows the recommendation counting the `.jar` files in
  the installation's `mods` folder, with **Use recommended** to take it.

## D-059 — Explaining why the game stopped

- **Status:** accepted (owner request, 2026-10-18) · **Affects:** the end of every
  game run, `RunSummary.diagnosis` and `signal`, the home screen, `open_instance_folder`
- **Sources.** Mojang's help on exit codes (0, 1, −1, −805306369 out of memory,
  −1073741819 access violation, −1073740791 native crash, −1073740777 D3DGear,
  −1073741515 missing DLL, 134, 139, 255); the crash rules of an open launcher and a
  log service (HMCL's crash analyser, Aternos' mclo.gs Codex); the
  loaders' own message files, read from their jars (Fabric Loader 0.19.5
  `Messages.properties`, Quilt Loader 0.30.1 `quilt_loader.properties`, the
  `lang/en_us.json` of Forge 47 and 52, NeoForge 21.1 and 26.3 and FancyModLoader 4
  and 12), so every version phrase, plural and variant is covered; the
  "Mod loading error" crash reports and Mixin; the
  Sodium wiki on graphics drivers and overlays; HotSpot's fatal-error file format.
  Twenty crashes were reproduced with this launcher on every loader and kept,
  without tokens and with neutral paths, as test fixtures in
  `src-tauri/src/launch/crash/fixtures`: wrong JVM option, Java 8 for 1.20.1; on
  Fabric a missing and a too-old dependency and a damaged jar; on Quilt a missing
  dependency (also with its error window failing), a duplicate and a damaged jar;
  on Forge 47 out of memory, a missing dependency and a damaged config; on Forge 52
  two missing dependencies; on Forge for 1.12.2 a missing dependency and a
  duplicate; on NeoForge 21.1 a missing dependency, a damaged jar and a Fabric mod;
  a modpack's memory check; a native crash with its `hs_err` file. Three normal
  games closed by the player are kept too, to show they stay unexplained.
- **When.** After a game process ends, unless the player pressed Stop. A clean exit
  (code 0) counts when the run wrote a crash report or JVM error file, or when its
  output holds a loader's dependency or duplicate message: Forge and NeoForge show
  their mod error screen and exit with 0 when it is closed (Forge for 1.12.2
  writes no report for it), so such a run is recorded as failed.
- **Questions on the console.** When a loader or modpack cannot show a dialog
  (NeoForge's loading window failing, tinyfd without zenity or kdialog), it asks
  on the console (`y/n:`, `[O]kay/[C]ancel:`, `[Y]es/[N]o:`). The game's input is
  empty, so the question repeated without end, the game never went on and its log
  grew by megabytes a second. After the third such question the launcher ends the
  game, keeps the repeats out of the log, and explains it from the lines above the
  question; when they name a cause (a damaged jar, a modpack's memory minimum),
  that is the cause and the question is a note.
- **What is read.** The game's last 2000 output lines (already redacted, log4j XML
  unwrapped); the crash report the game names in its output, else the newest
  `crash-reports/*.txt` written since the start; the newest `hs_err_pid*.log` in the
  game folder written since the start. Only the file name from the output is used,
  joined to the installation's own `crash-reports` folder; at most 2 MiB of each
  file. The JVM error file repeats the command line with the access token, so it is
  redacted on disk at once, and only its header and native frames are read.
- **Rules, first match wins:** JVM setup (unknown option, heap too large, missing
  main class) → Java version (class file version) → loader dependencies (missing,
  wrong version, Minecraft, Java or loader version, incompatible, duplicate,
  language provider), in each loader's words: Fabric's "Mod 'X' (x) 1.0 requires
  <any version phrase> of …", Quilt's "X [transitively] requires … of …, which is
  missing!" and "breaks with", "Duplicate mandatory mod ids", Forge's and
  NeoForge's "Mod ID: … Expected range" table and their screen texts ("Mod x
  requires y 1.0 or above / only supports / is incompatible with", then
  "Currently, y is …"), "is present in multiple files", and Forge for 1.12.2's
  `MissingModsException` and "Found a duplicate mod" → damaged files (zip errors,
  also Quilt's "as a zip file" and NeoForge's loading-window failure, with the
  jar's folder deciding between "replace the mod" and Repair) → damaged config
  (night-config, JSON) → memory (a modpack's own minimum; heap; native or
  system) → graphics (GLFW/OpenGL, RivaTuner, missing LWJGL
  natives) → native crash (overlay modules, then the graphics driver by module name
  for AMD, Intel, NVIDIA and Mesa, else the module and signal) → mods at run time
  (OptiFine, Mixin with the mod from its mixin config, entrypoints, mod
  construction) → disk full, locked files → crash report category (F3 + C, watchdog,
  ticking entity or block entity with type and position, world loading, any other
  description) → the exit code or signal alone (SIGKILL and 137 "killed", usually
  the out-of-memory killer; SIGTERM; SIGSEGV/SIGABRT and Windows status codes).
  Suspected mods come from the crash report ("Suspected Mods", `TRANSFORMER/<mod>@`).
  A mod skipped by the loader as made for another one (Fabric or Quilt on Forge
  or NeoForge, Forge on NeoForge, Forge or NeoForge on Quilt) is added as a note.
- **What the player sees.** The result carries the kind, the values for its text
  (mod names, versions, files, memory, module), up to five evidence lines, suspects,
  the report's file name and actions. The installation shows **Why the game
  stopped**: the cause, what to do, the suspects and notes, the exit code or signal,
  the evidence under a disclosure, and buttons that open the mods, config or crash
  reports folder (`open_instance_folder` takes only these names), its settings
  (memory, Java, loader version), Repair or Backups. Out of memory suggests the
  installation's recommended memory (D-058), or 2 GiB more. The last run on the home
  screen names the cause. Texts exist for all 37 kinds in English, Russian and
  Arabic; values a text needs but a crash lacks read as a shorter sentence.
- **Not done.** Uploading logs to a paste service; reading the full run log beyond
  the in-memory tail; guessing which mod caused a crash without a report naming it;
  naming the damaged jar when NeoForge's loading window does not; answering a
  console question for the player. NeoForge 26.3 was not run live (its
  installation needed a long repair); its messages are the same templates as
  NeoForge 21.1's.

## D-060 — Antivirus software and hangs

- **Status:** accepted (owner question, 2026-10-18) · **Affects:** IPC commands, the
  launch service, the mod loader installer, file replacement, error texts
- **Checked.** Every place the launcher starts a program or waits for one (the game,
  the Java check, the loader installer tools), its file replacement, its locks after
  being killed, and which work runs on the window's main thread.
- **The window never waits for the disk.** A plain `#[tauri::command]` runs on the
  main thread, which draws the window; file writes happen under the shared state
  lock, so a file held by antivirus software froze the whole window. The 22
  commands that touch files, the state or start programs are now
  `#[tauri::command(async)]` and run on a worker thread; only window commands stay
  on the main thread.
- **Nothing waits without end.** The Java check stops after 30 s (15 s for a chosen
  Java); the loader installer, which had no limit, now ends a step that prints
  nothing for 10 minutes (the tools print a line per file) and says that antivirus
  software may hold Java; a game asking on the console is ended (D-059); network
  requests time out after 15 s to connect and 60 s without data; Stop ends a game
  frozen by the system at once.
- **Files held for a moment.** On Windows, antivirus software opens a file it has
  just seen written, so renaming it fails with "access denied" or a sharing
  violation. Renames of new files (atomic writes, downloads, Java runtimes,
  backups, the trash) now try again for up to 3 s with growing pauses; other
  systems and other errors fail at once.
- **Named causes.** Windows' `ERROR_VIRUS_INFECTED` and `ERROR_VIRUS_DELETED` read as
  "antivirus software blocked the file"; Java that may not be executed (antivirus,
  AppLocker, a folder without execute permission) reads as "Java was not allowed to
  start" for the game, the Java check, a chosen Java and the installer, and does not
  ask for a repair; files missing again within 30 minutes of a successful repair
  point at antivirus software instead of asking for the same repair again.
- **Killed launcher.** The data folder lock is an OS lock released at exit; an
  interrupted install or repair is recorded as interrupted and continues from the
  files already checked; a game left running is recognized at the next start.
- **Not done.** A process the system cannot end (Windows protecting it, or stuck in
  the kernel) keeps its run open; the launcher cannot do more than ask the system.

## D-061 — Crash reports for the developers

- **Status:** accepted (owner request, 2026-10-18) · **Affects:** the launcher's logs,
  run logs, the diagnostic export, a new "send report" flow, Settings → Privacy, the
  new service `services/crash` at `https://crash.lapi.sh`. Changes docs/spec/07
  "Diagnostics and privacy": a report still leaves only when the player sends it, and
  anonymous statistics are a separate opt-in that is off by default.
- **Goal.** Collect the launcher's own failures and the games' crashes so that bugs
  get fixed and the crash analyser (D-059) learns the cases it does not explain yet.

### Stage 1 — kept on the computer

- **Launcher failures.** A panic writes `logs/launcher-crashes/<time>-panic.json`
  (version, OS, thread, message, place, backtrace, the last console lines). Errors in
  the window (`error`, `unhandledrejection`) go to the same folder as `frontend`
  records. A marker `logs/session.json` exists while the launcher runs; one left from
  an earlier start becomes an `unclean-exit` record (killed, hung, power cut).
- **Launcher log.** The console is also written to `logs/launcher.log`, rotated at
  2 MiB, five files, with the `diagnosticId` of every error shown to the player.
- **Game crashes.** A run that ended with an explanation keeps `diagnosis.json`
  (cause, details, analyser version) and redacted copies of the crash report and the
  JVM error file in its run folder.
- **Export.** The diagnostic ZIP also holds the launcher log's tail, the launcher
  failure records and these run files.

### Stage 2 — sent when the player chooses

- **Where.** After a game crash, the "Why the game stopped" panel offers **Send a
  report to the developers**; after a launcher failure, the next start offers the
  same. A preview lists every file with its text; the player can leave files out.
- **Scrubbing** on top of the existing token and path redaction: chat lines (`[CHAT]`
  and `<name> text`), the player's and every saved profile's name, UUIDs, IP addresses
  and server addresses, world names (`saves/<name>`), and the home folder. Mod names
  and versions stay: without them a report is useless.
- **Sending.** The report waits in `reports/outbox/` until the computer is online;
  on a metered connection only the player's own press sends it. One report is at most
  1 MiB gzip. Failed sends try again later; nothing is sent twice.
- **Identity.** A random install ID (Settings → Privacy, can be reset); never the
  Microsoft account, the player name or the IP address.
- **Service.** `POST /v1/reports` stores metadata in SQLite and the files gzip on
  disk, grouped by cause and signature (normalized top stack frames). Reports are
  kept 90 days. Rate limits in nginx and in the service. Developers read reports with
  the service's command line on the server (`lapi-crash groups|list|show`) or the
  "Crash reports" workflow summary; there is no public reading endpoint.

### Stage 3 — learning from them

- **Anonymous statistics**, a separate setting, off by default (on since D-071):
  cause, loader and versions, Minecraft version, OS, exit code, analyser version and
  signature, counted per day on the server, no install ID, no files.
- **"Did this help?"** under an explanation sends yes or no with its cause and
  signature, only when the player presses one of them; no install ID, no files.
- **Rules as data** (not built yet, the next step). The analyser's patterns move into
  a signed rule bundle the launcher downloads from the service, so a new mod message
  is explained without a launcher release; the built-in rules stay the fallback.
- **The loop.** Unexplained groups and "did not help" answers are reviewed; a
  scrubbed log becomes a fixture in `crash/fixtures`, a rule and a test.

### Server

- Debian 13 at `crash.lapi.sh` behind Cloudflare. Layout under `/home/www/lapi/`
  (room for other APIs): `crash/releases/<commit>/`, `crash/current` → the running
  release, `crash/data/` (database and reports, owned by the service user),
  `crash/crash.env`, and `nginx/crash.conf` included by nginx. The service runs as the
  system user `lapi-crash` under systemd on `127.0.0.1:8781`; nginx terminates TLS
  with a Let's Encrypt certificate and forwards to it.
- A one-time root script (`deploy/bootstrap.sh`, shared with D-062) creates the user, folders,
  systemd unit, nginx include, certificate and a sudoers entry that lets the deploy
  user only restart the service, test and reload nginx and run the service's command
  line. After that, the "Crash service" workflow tests, builds on Debian 13 and
  deploys over SSH on every change to `main`.
- **Since then** (D-062 to D-064 and the October 2026 audit): nginx files are root's,
  in `/etc/nginx/lapi`, installed by `bootstrap.sh` only; the deploy user may only
  restart each service and run its command line; the workflows build on Ubuntu 24.04
  and deploy from `main` only (also when run by hand), one deploy per service at a
  time, through the `production` environment. The units are
  hardened (system-call filter, `/home` hidden but the service's own folder), and
  `bootstrap.sh` puts a unit back if its service does not answer with the new one.

### Security

- **Only through Cloudflare.** nginx closes connections that do not come from
  Cloudflare's ranges (or the server itself), so the origin cannot be used
  around Cloudflare's protection; the client address is taken from
  `CF-Connecting-IP` only on those connections. The workflow checks from outside
  that the server refuses direct requests.
- **Limits before work.** Rate limits per address (an IPv6 /64 counts as one,
  and an IPv6 /48 gets five addresses' allowance, so one rented network cannot
  use up everyone's) and over all addresses together run before a body is read
  or unpacked; at
  most four reports are read at once; the unpacked body stops at 8.25 MiB (a
  gzip bomb ends there); stored reports stop at 5 GiB and the daily statistics
  rows at a fixed number, so a flood can fill neither memory nor disk. nginx
  adds 30 requests a minute and 8 connections per address and 2 MiB per body.
- **No reading over HTTP; nothing in public logs.** The repository and its
  workflow logs are public: the workflow prints counts per cause only, and
  reports are read on the server (docs/CRASH-REPORTS.md). The command line
  removes control and direction characters from what it prints, so a crafted
  report cannot drive a terminal.
- **Root stays root's.** nginx files live in `/etc/nginx/lapi`, which only root
  changes (bootstrap.sh); a deploy no longer writes them, since writing nginx
  config is as good as writing any file as root. `/home/www` and
  `/home/www/lapi` belong to root for the same reason. The deploy user may only
  restart the service and run its command line as `lapi-crash`.
- **No addresses kept.** The access log holds networks (`/24`, `/48`), and
  nginx does not log messages that would name a client.
- Validation as before: fixed words for causes and versions, UUID install IDs,
  file names without paths, sizes and counts bounded, SQL only with
  parameters, unknown fields dropped.

### Built

- Stages 1 and 2, the statistics and "Did this help?" of stage 3, the service and
  its deployment. A launcher failure is offered on the Home screen for 14 days, until
  it is sent or declined ("Don't send"). The rule bundle of stage 3 is left for later:
  first the reports show which rules are missing.
- **Amendment (owner, 2026-10-22, D-071):** the anonymous statistics are on by default
  and turned off in Settings → Privacy. One switch covers these crash statistics and the
  launcher's statistics of D-071; turning it off also erases the crash statistics that
  wait. Reports still leave only when the player sends them.

## D-062 — Download mirror at mirror.lapi.sh

- **Status:** accepted (owner request, 2026-10-18) · **Affects:** every download of
  the launcher (metadata and files), Settings → Network, the new service
  `services/mirror` at `https://mirror.lapi.sh`.
- **Goal.** The launcher depends on 17 hosts it does not run (Mojang, Adoptium and
  GitHub, Fabric, Quilt, Forge, NeoForge). When one is down, slow, blocked in the
  player's country, or has deleted a file, a feature stops working. Every one of
  those hosts gets a copy at our mirror, and the launcher switches to it by itself.
- **Owner's choices:** all hosts are mirrored (Mojang included; the owner accepts the
  licensing question); 1 TB of disk, changeable; traffic is not limited; a mode for
  players whose connection to an origin is blocked or very slow; Cloudflare caches.

### The mirror

- **Address.** `https://mirror.lapi.sh/<host>/<path>?<query>` stands for
  `https://<host>/<path>?<query>`, for the hosts of `src-tauri/src/net.rs` only
  (no open proxy): HTTPS, no user, no port, no `..`, redirects only to those hosts
  and GitHub's download hosts, public addresses only.
- **Files** (jars, libraries, asset objects, Java archives, Maven releases: anything
  whose content never changes at its address) are kept forever, also after the
  origin deletes them. nginx serves a kept file from disk; the service fetches a
  missing one once (concurrent requests wait for that one fetch), streams it while
  storing it, and keeps it. They are sent with `Cache-Control: immutable` so
  Cloudflare keeps them at its edge.
- **Metadata** (version lists, `maven-metadata.xml`, Forge promotions, Adoptium and
  Fabric/Quilt APIs: anything that changes) is fetched again after 10 minutes, and
  the last good copy is served for as long as the origin fails or returns 404.
- **Disk.** The quota (default 1000 GB) is the repository variable `MIRROR_MAX_GB`,
  written by the deploy; a full mirror and a disk with less than 5 GB free still
  pass files through, without keeping them. A bigger disk is mounted at
  `/home/www/lapi/mirror/data` (nginx serves kept files from there).
- **Warming.** A timer fetches what players will need before they ask: Mojang's
  releases (version files, client jars, libraries, asset indexes and objects), Java
  of every major version the launcher uses for every platform, and the loaders'
  installers and libraries for those releases. Snapshots and old versions are kept
  when first asked for.
- **Security** as for crash.lapi.sh (D-061): only Cloudflare may connect, root-only
  nginx files, a system user that writes only its data folder, limits on
  connections and on fetches per address and in total, no addresses in logs.

### The launcher

- **Routing per host.** A request goes to the origin first. It goes to the mirror
  instead when:
  - the origin fails twice in a row for this file (connection, DNS, TLS, timeout,
    5xx, 429), with the usual short pauses;
  - the origin answers 404 or 410 (the file may be deleted there);
  - the file does not match its checksum;
  - the origin is very slow: after 10 s a file still arrives at under 64 KiB/s
    and more than 1 MiB is missing.
    Two such failures of a host within two minutes mark the host as down for 10
    minutes: its requests go straight to the mirror, then one request tries the
    origin again. If the mirror fails too, the origin is tried again.
- **Slower than the mirror** (owner request, 2026-10-22). Every file the mirror
  copies reports its size and the time from asking to the last byte (without the
  disk), so many small files that each wait long count as slow too. When the
  origin's files of the last minute (at least 4, at least 1 s together) came at
  under 2 MiB/s, a trial starts: the host's next files take turns between the
  mirror and the origin until each side has brought 3, so both are measured at the
  same time on the same kind of files. If the mirror was at least twice as fast,
  the host's requests go to the mirror for 10 minutes and are then measured
  again; otherwise no trial for 5 minutes. A mirror failure ends a trial or gives
  the host back to its origin at once; a trial past 2 minutes is lost. Only the automatic mode compares; the catalog's R2
  reserve (D-066) and metadata are not measured.
- **Checks stay the same.** Every file with a checksum in its metadata is checked
  wherever it came from. Metadata without one (loader lists) comes from the mirror
  only when the origin failed; the console and the diagnostic export say which
  source served what.
- **Setting** (Settings → Network → Download source): **Automatic** (above,
  default), **Mirror first** (for blocked or slow origins; the origin is the
  fallback), **Original sites only** (never the mirror).
- The download manager says when a host is served by the mirror ("Down or slower
  than the mirror now: Forge. Downloading from the launcher's mirror.").
- Not covered: Microsoft sign-in (offline play, D-052, still works).

### Stages

1. The mirror service and its deployment; the launcher's routing and setting.
   **Built.**
2. Warming: `lapi-mirror warm`, run by `lapi-mirror-warm.timer` 15 minutes after
   boot and every 6 hours, through the running service. **Built.**
3. Statistics of mirror use per host (which origins fail most), in the counts-only
   overview of the workflow. Not built yet.

### Operating it

- The root step is the shared `deploy/bootstrap.sh` (crash and mirror): run once
  after the first deploy, and again when a workflow says the nginx files changed.
- Cloudflare: proxied DNS record for `mirror.lapi.sh`, SSL mode as for crash, and a
  cache rule "cache everything, respect origin cache headers" for
  `mirror.lapi.sh/*` (Cloudflare's default skips `.json` and extensionless asset
  objects).
- The quota is the repository variable `MIRROR_MAX_GB`; a bigger disk is mounted
  at `/home/www/lapi/mirror/data` (owner `lapi-mirror`, mode 0755).
- "Run workflow" on "Mirror service" with `task=reports` shows kept files per host,
  the quota and the free disk.
- The mod's Gradle builds in CI (Skins mod, Game test) use it the same way: a build
  that failed on a download runs once more with the Maven repositories of the mirrored
  hosts read through it (`.github/gradle/`). The origins always come first.

## D-063 — Launcher updates at upd.lapi.sh

- **Status:** accepted (owner request, 2026-10-18) · **Affects:** the launcher's
  start and exit, Settings → General, the download manager, the Package workflow,
  the new tool `services/upd` and `https://upd.lapi.sh`.
- **Goal.** Players get new versions by themselves, without a crash or a broken
  install on the way; publishing one is uploading a file and running a command.
- **Owner's choices:** download by itself and install when the launcher closes;
  a beta channel the player can choose (stage 2); no operating-system code signing
  for now; `.deb` installs are only told about a new version; publishing is
  "upload the release archive, run `lapi-upd publish`".

### Trust

- Every update is signed with the launcher's update key (Tauri updater, minisign
  Ed25519). The public key is built into the launcher (repository variable
  `UPDATE_PUBLIC_KEY`); the private key is only a GitHub secret
  (`TAURI_SIGNING_PRIVATE_KEY`, `TAURI_SIGNING_PRIVATE_KEY_PASSWORD`) and the owner's
  offline copy, never on the server. The launcher installs nothing whose signature
  does not match, and the signature must name the announced version
  (`requireSignedVersion`), so an old signed file cannot be passed off as new.
  The Tauri CLI's own signer does not write the version, so the Package workflow
  signs with `lapi-upd pack` (minisign, Tauri's key format, `version:` in the
  trusted comment); the builds themselves need no key.
- Losing the private key means installed launchers can no longer be updated; they
  have to be reinstalled by hand. A launcher built without the public key does not
  look for updates.

### Platforms

| Installed as                        | Update                                                                        |
| ----------------------------------- | ----------------------------------------------------------------------------- |
| Windows NSIS (x64, arm64), per user | the new installer runs passively after the launcher closes; no administrator  |
| macOS app (arm64, x64)              | the `.app` is replaced; the next start is the new version                     |
| Linux AppImage                      | the AppImage is replaced when its folder is writable, else the player is told |
| Linux `.deb`                        | the player is told, with a link to the new package (APT repository later)     |

### On the server

- No service process: nginx serves static files from `/home/www/lapi/upd`:
  `releases/<version>/` (installers, signatures, and the version's own manifest
  per installable platform, `<target>-<arch>-<bundle>.json`, for a return to it;
  kept, cached by Cloudflare) and `channels/<channel>/<target>-<arch>-<bundle>.json`
  (one manifest per platform and install type, cached one minute).
- Channels: `stable` and `beta`. Stable takes no pre-release (`1.2.0-beta.1`).
  Beta is never behind stable: publishing a stable version newer than beta's
  makes it beta's too (same rollout), and revoking it on stable takes beta back
  if beta got it from stable. Rollout, pause and resume are per channel.
- A manifest is Tauri's single-platform format (`version`, `notes`, `pub_date`,
  `url`, `signature`) plus our fields: `rollout` (percent of installs offered the
  version), `mandatory`, `revoked` (versions to leave even for an older one),
  `notesByLanguage`, and `download` (the page or package to open where the launcher
  cannot install itself).
- `lapi-upd` (run as the user `lapi-upd`):
  - `publish <version> [--channel stable] [--rollout N] [--mandatory]` reads
    `incoming/launcher-<version>-release.tar.gz` (built by the Package workflow:
    every platform's files, their signatures and `release.json`), checks that every
    platform is there, the checksums match and each signature verifies with the
    public key and names the version, that the version is newer than the channel's,
    then copies the files and replaces the manifests, each atomically;
  - `rollout <channel> <percent>`, `pause <channel>`, `resume <channel>`;
  - `revoke <channel> <version>`: the channel goes back to the previous version and
    launchers on the revoked one are offered it;
  - `status`.
- `https://upd.lapi.sh/download/` lists the current files for manual download.
- Security as for crash and mirror: only Cloudflare connects, nginx files are
  root's, no addresses in logs.

### In the launcher

- **When:** at start and every 6 hours; not offline (D-052); on a metered
  connection the download asks first (D-051).
- **Who is offered:** a stable random number per install (never sent) decides
  whether it falls within `rollout`; a revoked version is always offered the
  channel's version.
- **Download:** in the background, shown in the download manager like other
  downloads; the signature is checked when it ends.
- **Install:** when the launcher closes, unless a game or an install is running
  (then at the next close), or at once with "Restart now". Before the first start
  of a new version, the settings and installation records are copied to
  `backups/before-<version>/`.
- **Channel:** Settings → General → Updates → Versions: "Stable" (default) or
  "Beta: test new versions first". Changing it looks at once. Back to stable, the
  installed version stays until stable has a newer one (versions only go
  forward; a revoked one is left as above).
- **Going back when a new version does not start** (stage 2): before an update
  installs, the launcher writes `updates/pending.json` (from, to). Each start of
  the new version counts there, before its settings are read; its first start
  also copies the settings, accounts and installation records to
  `backups/before-<version>/`. A start is good when the interface has shown and
  the launcher stayed up 20 s, or closed normally after showing; then the file
  goes. After two starts without a good one, the third start downloads the
  version it came from (`releases/<from>/…json`, signed and checked like any
  update), puts back the copied records (the older version may not read what the
  newer one wrote), notes the return in `updates/rolled-back.json` and installs
  it. That version tells the player once and never takes the failed version
  again (`skippedVersion` in the settings); a newer one is offered as usual.
  Offline, the return waits for a start with the internet. Only updates the
  launcher installed are watched; a failure before our code runs at all (the
  executable does not open) cannot be caught from inside and needs `revoke`.
- **Seen by the player:** a header badge "Version X is ready" with what is new (in
  the interface language when the manifest has it) and "Restart now" / "When I
  close the launcher"; after the update, "What's new" once; Settings → General →
  Updates: "Download and install when I close" (default), "Only tell me", "Don't
  check". Errors say what happened and that the next check retries.
- **Not now:** an APT repository, Windows and macOS code signing.

### Publishing a version (owner)

`docs/RELEASING.md`: raise the version and add its notes, take the release
archive from the Package workflow, upload it to
`/home/www/lapi/upd/data/incoming/` and run `lapi-upd publish <version>`.

### Stages

1. Signing, manifests, `lapi-upd`, the server; the launcher's check, download,
   install on close, the interface, `.deb` notice, backup before a new version.
2. Automatic rollback after start failures; the beta channel in Settings (done).
3. APT repository; code signing.

## D-064 — Launcher account at oauth.lapi.sh, friends at friends.lapi.sh

- **Status:** accepted (owner request, 2026-10-18) · **Affects:** the Accounts
  tab, Play as, a chat icon in the corner, the new services `services/oauth`
  (`https://oauth.lapi.sh`, `/home/www/lapi/oauth`) and, from stage 2,
  `services/friends` (`https://friends.lapi.sh`, `/home/www/lapi/friends`).
- **Goal.** Our own account, the **launcher account**, made and used inside the
  launcher, for friends and chats and also as a player to choose in Play as.
  The domain names the service; the code and the interface say "launcher
  account".
- **Owner's choices:** nick, email and password with Cloudflare Turnstile; the
  sign-in pages are oauth.lapi.sh's own, in a window of the launcher with no
  address bar; the registration address and country are kept for recovery by
  hand; an external SMTP server comes later; nicks can change at any time;
  chat history is kept without end and without end-to-end encryption; backups
  later.

### Account service (`lapi-oauth`, stage 1)

- **Sign-in:** OAuth 2.1 authorization code with PKCE (S256) for the one client
  `launcher`. The launcher opens `/authorize` in its own window; the pages are
  served by oauth.lapi.sh, so the password never passes through the launcher's
  interface and Turnstile runs on its own domain. A finished sign-in goes to
  `https://oauth.lapi.sh/app/done?code=…&state=…`, which the window catches and
  never shows; the code works once, for a minute, with the matching verifier.
  `/authorize` refuses any other client, address, method or malformed
  challenge or state without redirecting anywhere.
- **Pages:** sign-in (nick or email), registration, password reset, email
  confirmation; English, Russian and Arabic (the launcher passes its language).
  Every value shown is HTML-escaped; no inline script, only Turnstile's and, on
  registration, our own `/static/register.js`, under a strict CSP
  (`frame-ancestors 'none'`, `form-action 'self'`); no caching, no referrer.
  Registration checks the nick (free, taken, not allowed) and the password while
  they are typed, through `POST /register/check`: only with an open sign-in and
  its cookie, 600 an hour per address, no email lookup, nothing kept. Forms carry a request ID and a CSRF token that must match the
  `__Host-flow` cookie (`Secure; HttpOnly; SameSite=Strict`).
- **Rules:** nick 3–16 of `A–Z a–z 0–9 _` (ASCII only, so no look-alike
  letters), unique without regard to case, a few reserved; email a plain
  address (no quotes, spaces or line breaks, so it cannot inject mail
  headers); password 10–128 characters, not among the 20 000 most used of that
  length, not containing the nick or the email's name.
- **Storage:** SQLite; passwords as Argon2id (19 MiB, 2 passes), at most 8
  hashed at once; codes, refresh tokens and email links as SHA-256 only; every
  query with bound parameters. Each account keeps its registration address and
  country (`CF-IPCountry`), its language, and a year of events (sign-ins,
  password and nick changes) with country only.
- **Tokens:** access tokens are JWT signed with Ed25519, 10 minutes, audience
  `account`, `friends`, `skins` (D-065), `mods` (D-066), `worlds` (D-068) and `reviews` (D-070), carrying the account ID, session, nick and whether
  the email is confirmed; the key is made on the server at the first start and
  never leaves it (`/.well-known/jwks.json` publishes the public half). Refresh
  tokens are random, live 60 days from their last use and rotate on every use;
  a used one presented again ends its session (a copy was stolen).
- **Abuse:** per-address limits (an IPv6 /64 counts as one) and overall ones:
  registration 5 an hour per address, sign-in 30. Turnstile always on
  registration and reset, and on sign-in after 3 failures from an address or 5
  against an account. No account is ever locked: its owner passes with the
  captcha. A wrong nick and a wrong password read the same and take the same
  time; "switched off" is said only to whoever knows the password. A reset
  request answers the same whether or not the address has an account.
  Registering an address that has an account says so (Turnstile and the limit
  keep that from being a lookup service).
- **Account API** (bearer access token, its session must be live): the
  account, nick change (any time; only 30 a day against scripts), password
  change (needs the current one, ends the other sessions, mails a notice),
  email change (needs the password, to be confirmed), confirmation letter
  again, sessions and ending them.
- **Mail:** letters wait in an outbox; a worker sends them through the SMTP
  server in `SMTP_URL` once it is set. Until then nothing is confirmed by mail.
- **Owner's commands on the server:** `lapi-oauth find <nick|email|id>`
  (account, registration address and country, sessions, events), `disable`,
  `enable`, `sign-out`, `set-email`, `confirm-email`, `counts`. The workflow's
  `counts` task prints numbers only.
- **Deployment** as for the other services: Cloudflare-only nginx, root-owned
  nginx files, a hardened systemd unit, no addresses in the access log. The
  Turnstile secret and the SMTP settings travel from GitHub secrets on standard
  input into `secrets.env` (0640), never on a command line or in a log.

### In the launcher (stage 1)

- Accounts tab, section **Launcher account**: "Sign in" and "Create account"
  open the oauth.lapi.sh window (no IPC, navigation only to oauth.lapi.sh and
  Turnstile); signed in: the nick, whether the email is confirmed, change nick,
  sign out. The refresh token is in the OS credential vault; the access token
  only in memory.
- **Play as:** the launcher account is one more player to choose; it plays
  under its nick without a Minecraft token (Minecraft's demo, as no-token
  profiles do, D-045).

### Friends (stages 2–4, friends.lapi.sh)

- A small chat icon in the corner opens everything: friends, requests in and
  out, add by exact nick, accept, decline, block (the blocked side is not
  told), chats with a friend and rooms of 2+ friends. Messages are text up to
  2000 characters, shown as plain text, stored without end, delivered over a
  WebSocket held by the launcher's core.
- friends.lapi.sh checks access tokens with the published key and knows only
  account IDs and nicks; it asks the account service for nicks over an
  internal API on 127.0.0.1:8793 that nginx never serves.
- **Requests:** by exact nick (case-insensitive). A request that meets the
  other side's is a friendship at once. Decline and cancel are not told.
  Block removes the friendship and requests; whoever is blocked is not told:
  their requests read as sent and reach nobody, and their messages in shared
  groups are hidden from who blocked them.
- **Rooms:** one per pair of friends, made when one of them first opens it;
  groups of an owner and 2–19 friends, at most 50 groups per owner. The owner
  adds friends, removes members and renames; anyone leaves; an owner who
  leaves passes the group to who joined first. Two people who are no longer
  friends keep their chat to read, and write no more.
- **Messages:** 1–2000 characters, control and direction characters taken
  out, at most 2 empty lines in a row; kept without end, no end-to-end
  encryption. Unread counts per room; a report of a message goes to the owner
  (`lapi-friends reports`).
- **Limits per account:** 20 requests an hour, 120 other actions an hour,
  messages 5 in 10 s and 600 an hour, 5 live connections.
- **In the launcher:** the core holds the WebSocket (`/v1/live`, Bearer token
  in the header, reconnects with a fresh token and backoff, ends when the
  account signs out), refreshes the lists on each event and sends the
  interface `friends://state` and `friends://message`. Since concept A
  (D-072) Friends is a row of the rail with a count of unread messages plus
  requests; it opens a side sheet at the window's inline end over a soft
  scrim. A press on the scrim, the focus or a press going anywhere outside
  the sheet and its row, or Escape puts it away, so nothing under it is
  reached while it is open. The sheet: Chats in two panes (the
  list with skin heads, times and previews; the conversation beside it,
  nothing opened, and so read, by itself), Friends (add by nick, message,
  invite, remove and block after a question, the blocked list, the chat
  settings), Requests (accept, decline, block, spam, cancel), "+" for a new
  group, and a chat with its history a page of 50 at a time, Enter to send
  and Shift+Enter for a new line, reports per message, a "⋯" menu (invite to
  the server the game is on, remove, block, leave a group) and the owner's
  member list. A world invite shows its world's name, version, mods and
  whether it is open as soon as it is in view (read only; at most 12 cards
  look by themselves in 10 minutes, so a chat full of codes cannot use up
  the player's looks, the others wait for a press on Look); joining the room
  takes a press, and only a code the service refused takes Join away. Message text is rendered as text only (no markup, no
  links), each message with its own direction.

### Stages

1. Account service and the launcher account (sign-in, registration, nick,
   Play as).
2. Friends: requests, accept, decline, block, the list; the chat icon.
3. Chat with a friend: WebSocket, history, unread.
4. Rooms of 2+ friends.
5. Later: two-factor codes, mails about new sign-ins. (Reports of messages came with
   stage 4; daily database copies, account deletion and export, moderation and online
   status with the October 2026 work below.)

### Deletion, export, moderation and online status (October 2026)

- **Deleting the account** happens on the account service's own page (`/delete`,
  opened in the account window from Accounts → _Delete account…_): nick or email and
  the password, behind the captcha when it can be solved, then a one-time confirmation.
  Everything kept about the account goes; its address gets a last letter; the nick and
  address are free again. The page ends on `/app/deleted`, which the window watches to
  forget the account here. The account service lists deleted accounts on its internal
  port for 90 days; friends cleans them out within 15 seconds (their friends, requests,
  blocks, messages, rooms of two, place in groups, reports they filed). Reports about
  their messages keep their copy as evidence. Skins, worlds, mods and reviews do the
  same within 15 seconds: worn skins; the rooms the account owns with all they hold,
  its places in other rooms, devices, invites, tickets and relay keys (open admissions
  are revoked and name no account); its requests to the catalog and its reports; its
  reviews' files and a ban of the account. Its own packs and reviews stay, made
  anonymous (Ai, 2026-10-21): a pack loses its author's name and belongs to no account,
  so no one can change it and players can still report it; a review loses its link to
  the account. What the player wrote in them stays.
- **Sign-outs reach every service.** Friends, skins, worlds, mods and reviews ask the
  account service every 15 seconds which sessions they saw ended (signed out, password
  reset, account switched off or deleted) and refuse their access tokens from then on,
  though the tokens' signatures hold until they run out. A token of an account
  deleted lately is refused even from a session the service never saw.
- **Export:** Accounts → _Download my data_ saves one JSON file with what the account
  service (`GET /v1/me/export`), friends, worlds, the mods catalog and reviews (each
  `GET /v1/export`, ten an hour) keep about the account, asked by its own session. A
  service that does not answer is `null` in the file; the others still come. Worlds
  gives the account's devices, its rooms, where it is a member, its invites, the
  sessions its devices hosted and its admissions (tickets and relay grants last minutes
  and are left out); the catalog its requests to add, its packs and their versions, and
  its reports; reviews what it wrote, the answers and its files' names and sizes, but
  not the owner's notes or spam marks.
- **Moderation** is the owner's, on the server only (docs/MODERATION.md): reports keep
  a copy of their message; `resolve`, `remove-message`, `mute <hours>`, `ban [days]`,
  `lift`. A removed message leaves every member's chat; a muted account reads but
  writes nothing; a banned one gets nothing and its live connections close. The running
  service tells within 15 seconds.
- **Online status:** friends see who has the launcher open and what they play
  ("Minecraft 1.21.1 · Fabric", sent by the launcher while the game runs), unless the
  account hides it (Friends tab → _Show friends that I am online_). Nothing of it is
  stored: it lives with the live connections.

## D-065 — Our own skins: skins.lapi.sh and a mod for every loader

- **Status:** accepted (owner request, 2026-10-19) · **Affects:** the Accounts tab
  (launcher account), installation launch (a mod or a Java agent added to the game),
  the new service `services/skins` (`https://skins.lapi.sh`, `/home/www/lapi/skins`),
  the new `mod/` build, docs/spec/01 (skin editing stays out of scope: players upload a
  finished PNG).
- **Goal.** Players of launcher accounts (D-064) wear their own skin and cape, HD and
  animated, and see each other's, looked up by nick, in every way the launcher runs the
  game: Fabric, Quilt, NeoForge, Forge and plain Minecraft without a loader. The code is
  ours: CustomSkinLoader (GPL-3.0) and authlib-injector (AGPL-3.0) were read for
  behaviour only, as Prism was for sign-in; nothing is copied.
- **Game versions (owner's choice):** 26.x, 1.21.x and 1.12.2 first (2026-10-19), then
  every release down to 1.14.4 and Forge 1.8.9 and 1.7.10 (2026-10-21, "масштабно
  расширь"); of snapshots only the newest one (2026-10-23: an older snapshot has no
  players, so a new one replaces it in `mod/mc-snapshot`, `builds.json` and `BUILDS`).
  What mod 0.8.0 covers, as `mod/builds.json` lists it (updated 2026-10-23):

  | Game version    | Fabric (also Quilt) | NeoForge               | Forge                    | Without a loader |
  | --------------- | ------------------- | ---------------------- | ------------------------ | ---------------- |
  | 26.4-snapshot-5 | yes                 | none for snapshots     | none for snapshots       | Java agent       |
  | 26.1–26.3       | yes                 | yes                    | yes                      | Java agent       |
  | 1.20.6–1.21.11  | yes                 | yes                    | yes                      | Java agent       |
  | 1.20.5          | yes                 | yes                    | no Forge then            | Java agent       |
  | 1.20.2–1.20.4   | yes                 | yes                    | yes                      | Java agent       |
  | 1.20.1          | yes                 | yes (runs Forge's jar) | yes                      | Java agent       |
  | 1.17.1–1.20     | yes                 | no NeoForge then       | yes                      | Java agent       |
  | 1.17            | yes                 | no NeoForge then       | no Forge then            | Java agent       |
  | 1.16.5          | yes                 | no NeoForge then       | yes (Mixin)              | Java agent       |
  | 1.16.1–1.16.4   | yes                 | no NeoForge then       | yes (JavaScript coremod) | Java agent       |
  | 1.16            | yes                 | no NeoForge then       | no Forge then            | Java agent       |
  | 1.15.2          | yes                 | no NeoForge then       | yes (JavaScript coremod) | Java agent       |
  | 1.15–1.15.1     | yes                 | no NeoForge then       | yes (JavaScript coremod) | Java agent       |
  | 1.14.4          | yes                 | no NeoForge then       | yes (JavaScript coremod) | Java agent       |
  | 1.14.2–1.14.3   | yes                 | no NeoForge then       | yes (JavaScript coremod) | Java agent       |
  | 1.14–1.14.1     | yes                 | no NeoForge then       | no Forge then            | Java agent       |
  | 1.13.2          | no Fabric then      | no NeoForge then       | yes (JavaScript coremod) | Java agent       |
  | 1.12.2          | no Fabric then      | no NeoForge then       | yes (ASM coremod)        | Java agent       |
  | 1.11.2          | no Fabric then      | no NeoForge then       | yes (ASM coremod)        | Java agent       |
  | 1.10.2          | no Fabric then      | no NeoForge then       | yes (ASM coremod)        | Java agent       |
  | 1.9.4           | no Fabric then      | no NeoForge then       | yes (ASM coremod)        | Java agent       |
  | 1.8.9           | no Fabric then      | no NeoForge then       | yes (ASM coremod)        | Java agent       |
  | 1.7.10          | no Fabric then      | no NeoForge then       | yes (ASM coremod)        | Java agent       |

  Everywhere: skins and capes by nick, HD up to 512 and animation. 1.7.10 has no slim
  arms (the game has none). A game version not in the table, an older snapshot, a
  pre-release or a release candidate gets no jar and starts without it. The agent
  rewrites the same game method the mods hook: one for 26.x and the snapshot, whose names
  are not obfuscated, and one per release before 26.1, built in Mojang names (MCP names
  for 1.12.2 and older, on the Forge jar's game layer) and remapped to that release's
  names; 1.13.2's agent and Forge jar are in MCP names too, on a game layer of their own.
  1.14 to 1.14.3 have no Mojang names either, and their agents are the 1.14.4 one
  remapped through Fabric's intermediary names, which those releases share. OptiFine on
  its own (D-039) runs the agent too, from mod 0.8.0 on: OptiFine starts the game
  through launchwrapper, whose class loader the agent joins. Its capes stay where
  OptiFine shows one.

  Played in the real game by the Game test (docs/GAMETEST.md) every night: 26.3,
  1.21.11, 1.21.1, 1.20.1 and 1.12.2 with every loader they have and without one, and
  OptiFine on its own in 1.21.1, 1.20.1 and 1.12.2; and in turn three more of the
  versions it has bots for (the snapshot, 1.21.10, 1.21.8, 1.21.4, 1.20.6, 1.20.4,
  1.19.4, 1.19.2, 1.18.2, 1.17.1, 1.16.5, 1.15.2, 1.14.4, 1.10.2, 1.8.9 and 1.7.10)
  without the mod set, so each is played within six nights.

- **Textures:** PNG only. A skin frame is 64×64 × k (k = 1, 2, 4, 8, so up to 512×512;
  64×32 × k, the layout before 1.8, only as one frame: the mod turns it into the
  square one), a cape frame 64×32 × k. Animation is a vertical strip of
  1–32 frames with one duration or one per frame (50–10 000 ms). Skins up to 2 MiB, capes
  up to 1 MiB. The service decodes with its limits set before decoding, re-encodes and
  stores only its own file, named by its SHA-256; the mod checks size and hash again.
- **Whose skin is shown:** a player whose profile already carries textures (a Microsoft
  account on an online server) keeps them. Ours is used only when the profile has none
  and its UUID is not a Microsoft one (version 4, random): offline players (version 3)
  and the launcher's own players (version 5) get ours, so a launcher account named like
  a Microsoft player never replaces that player's skin.
- **Service (`lapi-skins`):** `GET /v1/profile/{nick}` and `POST /v1/profiles` (up to
  50 nicks) are public; `GET /v1/textures/{sha256}` serves the PNG, cached for good;
  `GET /v1/me`, `PUT`/`DELETE /v1/me/skin` and `/v1/me/cape` and `GET /v1/export` take
  the launcher account's access token (audience `skins`, added to the account
  service's tokens). Nicks are resolved through the account service's internal API, so
  a nick change moves the skin at once; deleted accounts are cleaned out within 15
  seconds. Uploads: 10 an hour per account; `GET /v1/me` and the answers to `PUT` and
  `DELETE` say how many are left (`uploads: {left, reset}`, reset in seconds). The
  wardrobe (`/v1/me/wardrobe…`, below) takes the same token. Nothing about players
  goes into logs; `counts` prints numbers only; `remove <account> [skin|cape]` is the
  owner's moderation command and also takes that texture out of the account's
  wardrobe.
- **Mod (`mod/`):** one Gradle build. `core` (plain Java 8: the API client, a disk cache
  by hash, PNG checks, frame timing, the whose-skin rule), per game version a `common`
  layer (Mixin hooks in Mojang names, the same source for every loader), thin entry
  points per loader, a Forge 1.12.2 coremod, and per version an `agent` (a `premain` that
  rewrites `PlayerInfo.getSkin()` and reuses the same game layer). One jar per loader and
  version; the launcher picks the right one. The game never waits for a skin: profiles
  are asked in batches of up to 50 (`POST /v1/profiles`, so 100 players cost two
  requests) and textures load on four background threads.
- **In the launcher (later stage):** Accounts → Launcher account: upload a skin (model
  default or slim, frame duration) and a cape, preview, remove. Installations get the
  matching jar in `mods/` (Fabric, Quilt, NeoForge, Forge) or `-javaagent` (no loader),
  checked by SHA-256 like other downloads, with a switch per installation. The mod reads
  the API address from a system property and falls back to `https://skins.lapi.sh`; the
  game never gets a token.
- **Stages:**
  1. the service;
  2. upload in the launcher;
  3. core + Fabric 1.21.x with plain skins;
  4. HD;
  5. animation;
  6. NeoForge, Forge, 26.x and 1.12.2;
  7. the agent;
  8. automatic install by the launcher, tested on a Windows package.
- **Automatic install (stage 8):** the jars come from the GitHub release
  `skins-mod-v<version>`, pinned by SHA-256 and size in the launcher
  (`src-tauri/src/skins_mod.rs`), downloaded once (from upd.lapi.sh only, see the
  amendment below) into
  `shared/launcherskins` and checked again at every launch; with a loader the jar is
  copied into `mods/` (other `launcherskins-*.jar` there are removed), without one it is
  added with `-javaagent`.
  The switch "Launcher skins" per installation is on by default; off removes our jars
  from `mods/`. Exact game versions only (the table above); a jar that cannot
  be had never stops the game, it starts without it.
- **Server setup (owner):** a DNS record and certificate for skins.lapi.sh, then
  `deploy/bootstrap.sh` once, as for the other services.
- **Amended 2026-10-22 (owner request: no GitHub for players, only our domain):**
  players download the jars only from our update server, at
  `https://upd.lapi.sh/skins-mod/<version>/<file>`, never from GitHub, and no other
  host passes the launcher's check for them. The GitHub release stays where CI
  builds and keeps them: the workflow "Skins mod list" uploads a release's jars
  with its `SHA256SUMS`, `lapi-upd skins-mod-jars <version>` checks every jar
  against its sum before it writes any and never changes a jar already published,
  and the workflow downloads each one back from upd.lapi.sh before a list goes out.
  `lapi-upd skins-mod <channel>` takes a list only when all its jars are in place.
  Channel `none` uploads the jars alone (a version built into the launcher that no
  list names). nginx serves `/skins-mod/<x.y.z>/launcherskins-*.jar` only, cached
  for good; the new location needs `deploy/bootstrap.sh` once on the server.

### Updates without a launcher update

- **Status:** accepted (owner request, 2026-10-21). A new mod version reaches players
  without a new launcher; the built-in table stays the fallback.
- **The list:** `https://upd.lapi.sh/v1/<channel>/skins-mod-list.json` (stable,
  beta), served by nginx like the update manifests. It is `{list, signature}`:
  `list` is the exact signed text of `{kind: "skins-mod", channel, serial, version,
rollback, jars: [{target, games, file, sha256, size}]}`, `signature` a minisign
  signature with the launcher's update key (D-063) whose trusted comment names
  `kind:skins-mod`, the channel and the serial. The serial is the signing time in
  seconds. The jars stay on the GitHub release `skins-mod-v<version>`; players get
  them from upd.lapi.sh (amendment above).
- **Signing:** only in CI (workflow "Skins mod list"), from the release's
  `SHA256SUMS`, its jars and `mod/builds.json` (the launcher's `BUILDS` as data; a
  test keeps the two the same). The key never goes on the server: `lapi-upd
skins-mod <channel>` there only checks the signature against the public key and
  that the serial is newer than the channel's, then puts the file in place.
- **Channels:** a release (Skins mod workflow, publish) goes to beta at once; a
  green nightly game test gives it to stable when the tested commit's mod code is
  the release's and beta has it. **Going back** is one "Run workflow" of "Skins mod
  list" with the older version and `rollback`: a newer serial for an older
  version.
- **In the launcher** (`src-tauri/src/skins_mod/list.rs`): asked with the update
  check (at start and every 6 hours, not offline, not when checks are off; a
  manual check asks too) for the player's update channel. Taken only when the
  signature verifies with the built-in key, the comment names this kind, channel
  and serial, every entry is in bounds (names like ours, SHA-256, size up to
  16 MiB, plain game versions, at most 256 jars and 512 KiB) and the serial is not
  lower than the kept list's, so an old signed list cannot be served again. Kept
  per channel in `shared/launcherskins/list-<channel>.json` and checked again at
  each launch. The list's jar is used when its version is not older than the
  built-in `MOD_VERSION` (the built-in jar fills a game version the list lacks),
  or always when it goes back (then only its own jars). No key built in, no list,
  a damaged list: the built-in table, as before. Nothing about the player is sent,
  and nothing stops a launch.

### The skin and cape screen

- **Status:** accepted (owner, 2026-10-21, with all five proposed defaults). Stage 1
  of 4 is built: the screen in the launcher; the wardrobe on the server (stage 2) and
  the head avatar, skin by nick and refresh in a running game (stage 3) follow; stage
  4 (Microsoft skin, launcher capes, ears, an editor) only when the owner asks.
- **Where:** a block in Accounts → Launcher account and a tab "Skins" in the main row
  (the same component, `src/features/skins`). The account window (D-064) has no IPC, so
  the screen lives in the main window.
- **The character:** skinview3d 3.4.2 (MIT, three.js), bundled and loaded only when the
  screen opens; nothing comes from the network. Textures reach the page as `data:`
  URLs from the core (the CSP has no `blob:`), are cut into one canvas per frame and
  change by the wall clock, as the mod does. Animations (standing, walking, running,
  waving, crouching, hitting, flying on elytra), four views (front, 3/4, back,
  turning), four backgrounds, the second layer, the nick over the head, cape or
  elytra, "before and after" while a file is tried on, "as seen without the mod" (a
  stand-in for the game's own skin, drawn by us: Mojang's may not be copied) and a
  1024×1024 PNG with a transparent background, saved through a native dialog. Arrow
  keys turn it, a double click resets it, the pointer over it stops it turning. No
  WebGL, a lost context or a failed start: flat front and back drawings instead. The
  OS setting "reduce motion" stops everything that moves by itself.
- **A new file:** picked in a dialog or dropped on the skin or cape card. The window's
  own drop handler stays off (D-070 needs it off), so the page reads the dropped file
  (`.png`, `.gif`, `.apng` by name, up to 8 MiB) and hands the bytes to the core
  (`skins_take`), which checks them as it checks a picked file. A GIF or an APNG
  becomes a vertical strip with its frame durations; the screen says what the import
  did (identical frames merged, every n-th frame kept, durations raised to 50 ms).
  The arms are guessed from the file. Before it is worn the player sees each frame
  and can hold one, give one duration to all or one per frame (50–10 000 ms).
- **Import limits (core, `src-tauri/src/skins_import.rs`):** the format is read from
  the bytes, never the name. The work is bounded before it starts: at most 8 MiB, then
  the picture's size from the header (a skin or cape size), then at most 256 frames in
  the file (an APNG's announced count is checked before any frame is read) and 16 Mi
  pixels for all of them together. Each frame is drawn onto the whole picture with its
  disposal and blending, as browsers draw it; frames that look the same in a row
  become one, more than 32 are thinned to every n-th, and durations are brought into
  50–10 000 ms (a GIF delay of 10 ms or less counts as 100, as in browsers). A broken
  file is refused, never half shown.
- **Saving the picture:** `skins_save_picture` (async: it opens a native dialog) takes
  only a PNG that decodes fully, at most 2048 pixels a side and 16 MiB; the offered
  name is the nick reduced to letters, digits, `_` and `-`. The log gets a fixed line.
- **Heads and skins by nick (2026-10-22, for the redesign, D-072):** `skins_heads`
  gives the heads of up to 200 players for friends, rooms and chats: the face of the
  skin's first frame with the hat over it (a skin of the layout before 1.8 whose hat
  area has no transparency shows its face alone, as in the game), a square PNG 8
  pixels per 64 of the skin's width. Nicks that cannot be one (3–16 Latin letters,
  digits, `_`) are never sent; the rest go 50 a request to `POST /v1/profiles` and are
  kept a minute, the heads by their skin's hash. `skins_by_nick` takes the skin or
  cape another player wears as a picked file, with its owner's arms and frame
  durations, to try on and wear. A worn skin is public already (every game with the
  mod shows it, as in Minecraft), and the board the owner chose (D-072) has this
  button. Textures come only by hash and are checked against it. Nicks never go into
  the log or an address beyond the profile lookup.
- **The wardrobe on the server (stage 2, 2026-10-22, for the redesign, D-072):** an
  account keeps up to 30 skins, 10 capes and 20 sets (a skin, a cape or both), the
  same on every computer. An item is a texture by hash with its arms, frame
  durations, a name (up to 32 characters, trimmed; no control, zero-width or
  direction characters), a favourite mark, when it was added and when last worn; one
  texture is kept once per account. `GET /v1/me/wardrobe` gives the profile, the
  uploads left, the items, the sets and the limits. `POST /v1/me/skin` (or `cape`)
  adds a PNG without wearing it (an upload, counted like one; a full wardrobe refuses
  it before it is counted), `POST /v1/me/wardrobe/save` keeps what is worn and
  `POST /v1/me/wardrobe/sets` keeps it as a set (neither is an upload). Items and sets
  are renamed or marked with `PATCH`, removed with `DELETE` and put on with
  `POST …/{id}/wear`: 120 an hour, never counted as uploads; putting on a set takes off
  what it lacks. Other changes: 600 an hour. A full wardrobe answers `wardrobe_full`,
  saving with nothing worn `nothing_worn`, a bad name `bad_name`; ids are 32 hex
  characters, anything else is not found. A texture stays on the server while it is
  worn or in a wardrobe. `GET /v1/export` includes the wardrobe; deleting the account
  deletes it. In the launcher: `skins_wardrobe` reads it, `skins_keep` uploads the
  picked file into it, and `skins_wardrobe_change` keeps what is worn (or as a set),
  renames, marks, removes and puts on; each answers with the whole wardrobe, checked
  as worn textures are (IDs, names by the same rule, at most 128 items and sets).
  `skins_mine` also gives the uploads left. Previews are kept by hash, 64 of them.
- **Heads of Microsoft accounts (2026-10-22, for the redesign, D-072):**
  `skins_microsoft_heads` gives the head of each Microsoft account signed in on this
  computer (the rail and Accounts), by profile ID. Only those IDs are asked, without
  any token, from Mojang's public session server; the skin is then fetched from
  textures.minecraft.net (only `/texture/<hex>` there, whatever URL the answer
  names) and cut as our heads are. Both hosts are a source category of their own
  (`MojangSkins`: HTTPS, public addresses only, not on the mirror). The window's CSP
  stays as it is: the page gets a `data:` URL. Mojang's default skin, or no answer,
  gives none and the page draws the default; heads are kept ten minutes.

## D-066 — Catalog of mods at mods.lapi.sh

- **Status:** accepted (owner request, 2026-10-19), stages 1–6 · **Affects:** the new service `services/mods` (`https://mods.lapi.sh`, files at
  `https://files-mods.lapi.sh`, `/home/www/lapi/mods`), the account service's token
  audience. The full design: the project's design document "Вкладка Моды и каталог".
- **Goal.** A Mods tab with modpacks, mods, resource packs, shaders and worlds that does
  not depend on Modrinth's and CurseForge's servers being up: what players add to our
  catalog is kept here for good (pages, pictures, files), with permission from both.
- **Owner's choices:** the API at mods.lapi.sh, files at files-mods.lapi.sh behind
  Cloudflare's ordinary cache, no proxies toward players; a pool of proxies toward the
  sources; no CurseForge API key, so its site's own addresses; our own server; every
  file kept; any signed-in player may add; at once the newest version for each game
  version and loader, any other version on its first request and then for good, the
  catalog saying which versions are kept.
- **Adding:** a player gives an address (Modrinth project, version or file; CurseForge
  comes in stage 4) or a name; a name offers up to five projects to pick from. The
  service fetches the project, its members and all its versions' metadata, then the
  files of the newest release per game version and loader, required dependencies (to
  the end of the chain, each project read at most once a day so a loop ends; 200 per
  project), the icon, the gallery and the pictures in the description (a plain `http:`
  address is fetched over HTTPS; pictures are asked with a browser's `<img>` headers,
  since imgur answers a page request with a redirect to its own page), which then point
  at our copies. Description pictures may come from any public host, since authors use
  many (owner, 2026-10-21): a name, never an address or an inner network's name, that
  resolves to public addresses only (checked also when a proxy connects), every
  redirect checked the same way, at most 30 a project and 20 MB each, kept only when
  the bytes are PNG, JPEG, GIF or WebP, paced as picture hosts. Players only ever load
  our copies. Files and pages still come from listed hosts only. Work runs from a queue in the database (retries with
  back-off, survives restarts). Limits: 20 requests an hour and 100 a day per account,
  5 GB a day of files brought per account, 60 an hour per address; an older version's
  files 30 an hour per address.
- **Sources:** everything is fetched with curl-impersonate (lexiforest, Chrome 150's
  fingerprint), as a process with its arguments one by one and no shell, only HTTPS to
  listed hosts, redirects followed by us one hop at a time and checked again, names
  resolved to public addresses only, sizes bounded. Exits are the proxies of the secret
  `LAPI_MODS_PROXIES` plus the direct connection, each paced per site (Modrinth 200 a
  minute, CurseForge 60, picture hosts 120) and rested for an hour after a refusal or a
  Cloudflare challenge. A proxy's address never appears on a command line or in a log.
- **Storage:** files by SHA-256 under `data/blobs/ab/cd/<sha256>`, written to a part,
  hashed, checked against the source's SHA-1 and SHA-512 and size, synced and renamed;
  a file that does not match is not kept. SQLite (`data/db`, 0700) holds projects,
  versions, files, dependencies, pictures, requests and the queue, with FTS5 search.
  Quota: the repository variable `MODS_MAX_GB` (default 500), and 5 GB always left free.
- **Serving:** nginx serves `/f/<sha256>/<name>` (always a download) and
  `/i/<sha256>.<png|jpg|gif|webp>` straight from disk, immutable, `nosniff`, CSP
  `sandbox`. The type of a picture is taken from its bytes, never from the source.
  Small copies (icons 256 px lossless WebP, gallery 512 px JPEG) are made by
  `lapi-mods thumb` in a process of its own with bounded size and memory. The launcher
  shows no picture from another site, so players' addresses go only to us. The projects
  a name finds come with their icon and poster (Modrinth's featured gallery picture),
  kept here first like any picture.
- **New versions:** the service checks the projects it keeps at their source again in
  the background (every 10 minutes it queues up to 100 that are due, longest waiting
  first): a project players wanted in the last 30 days (its page opened, or found by
  hash as installed) once a day, any other once a week (Ai, 2026-10-20). A page opened
  more than a day after the last check queues one at once. Only the time a project was
  last wanted is kept, to the hour, never who wanted it. A check reads the project like
  an import: new versions come in, and the newest per game version and loader is kept.
- **A project's versions** come whole (up to 2000) and the page filters them by game
  version, loader and type and orders them newest or oldest first; it starts with the
  chosen installation's game version and loader. A version the catalog keeps says
  **In the catalog**; one it does not has **Save to the catalog**.
- **API:** `GET /v1/search`, `/v1/projects/{id|slug}`, `/v1/projects/{id}/versions`,
  `/v1/versions/{id}` (each version says `saved`: `all` kept, else fetched on request),
  `POST /v1/versions/{id}/fetch`, `GET`/`POST /v1/versions/{id}/pack` (stage 3),
  `POST /v1/lookup` (by SHA-1 or SHA-256); with the
  launcher account's token (audience `mods`): `POST /v1/imports`, `GET /v1/imports`,
  `GET /v1/imports/{id}`, `POST /v1/imports/{id}/choose`.
- **Plan:** 1 this service and its deploy; 2 the launcher's tab; 3 modpacks (installing
  `.mrpack` and CurseForge packs, other launchers' manifests); 4 CurseForge through its
  site; 5 more formats; 6 upkeep (refreshing, reports, removal on request).
- **Owner actions:** docs/MODS-CATALOG.md.

### In the launcher (stage 2)

- **Mods tab** next to Installations: Mods, Resource packs, Shaders, Data packs, Modpacks
  (to look at; installing one as a new installation is stage 3), Installed and My
  requests. With an installation chosen, the search starts at its game version and, for
  mods, its loader; a project's versions can be limited to those that fit it.
- **Only through the core.** Every call goes through Rust commands (`mods_*`); answers are
  bounded in size and checked (our IDs, closed lists of kinds, loaders and sources,
  lengths). Descriptions and changelogs come as a tree of safe nodes made from the
  Markdown in the core (pulldown-cmark); CurseForge's descriptions are HTML only and are
  read as HTML. HTML is never drawn: a fixed set of elements (paragraphs, headings,
  lists, tables, quotes, code, emphasis, links, pictures) becomes the same nodes,
  attributes are read only for a link's or picture's address, anything else gives its
  text, and scripts, styles and comments give nothing; a link is kept only when it is `https:` and opens in the browser
  after the player confirms; a picture only when files-mods.lapi.sh serves it. The CSP's
  `img-src` allows exactly that host.
- **Adding to an installation:** a plan made in the core from the catalog's answers. A
  mod must list the installation's game version and loader (Quilt also takes Fabric's);
  a resource pack, shader or data pack that does not only gets a warning. Required
  dependencies are added (the version the dependency names, else the newest release
  that fits; to the end of the chain, each project once, 200 files at most or no plan);
  one with no fitting version, an incompatible project
  already there, or a file of the player's at the same name stops the plan. What is in
  the folder already counts, the player's own files too when the catalog recognizes them
  by SHA-1: a dependency there is not brought again, one turned off is turned on with
  the install, the player's own copy of the chosen mod is replaced (two copies of a mod
  stop the game), and an incompatible project is found among them as well. Files the
  catalog does not keep yet are asked for and waited on (20 minutes at most). Every file
  comes from files-mods.lapi.sh through the download executor, checked against its
  SHA-256 before it is put in place; the traffic counts as "Catalog of mods" and the
  launcher's mirror is not used for it. One change at a time per installation; not
  while the game runs, and the game does not start while one runs.
- **`content.lock.json`** next to the installation records what the launcher put there
  (path, SHA-256, project, version, the projects that version requires). An update replaces only its own file, or the
  player's file they chose to update; a turned-off file stays off.
- **Installed:** mods, resource packs, shaders and the data packs the launcher added;
  on and off by the `.disabled` suffix, removal after a confirmation, updates of what the
  launcher put there and of the player's own files the catalog recognizes by SHA-1
  (`POST /v1/lookup`), one at a time or all. Before a mod is turned off or removed the
  launcher says which mods turned on require it (from the lock, else the catalog; the
  lock alone offline); removing also offers to remove the dependencies the launcher
  added that nothing else needs, a mod turned off included. Paths from the interface must be in
  `mods/`, `resourcepacks/`, `shaderpacks/` or `saves/<world>/datapacks/`, one name deep,
  never a link.
- **Add to the catalog** and **My requests** use the launcher account's token; signed
  out, the launcher offers to sign in. A request in progress is asked about every 3 s.

### CurseForge (stage 4)

- **First, how the site answers the server.** CurseForge has no key here, so the catalog
  uses the site's own API and forgecdn; its project pages and search sit behind a
  Cloudflare check that may refuse data-centre addresses. `lapi-mods probe` asks one
  public project (JEI) through every exit, direct and each proxy, once per check: the
  site's file list, one file, its changelog, its download link, dependencies, the
  project and search in the API, and the project and search pages (whether they carry
  their Next.js data). It prints our words and numbers only, proxies numbered; Actions
  → Mods service → `task=probe` runs it. Its answer decides whether project pages need
  residential or mobile proxies.
- **First probe (2026-10-19), direct from the server:** the site's file list, one file,
  its changelog, dependencies and the download link (307 to forgecdn) all answer; the
  project and search in the site's API are 403; the project page is 200 with no
  Cloudflare check, and search answers with a redirect. Every proxy failed at the
  proxy handshake (curl's code 97). The probe now names which kinds of page data it
  finds (the Pages Router's `__NEXT_DATA__`, the App Router's `self.__next_f`, JSON-LD,
  Open Graph), follows one redirect within the site, and asks a refused proxy again as
  `http://` and `socks5h://` to tell a wrong scheme from a dead proxy.
- **Second probe:** every proxy answers as `http://` and as `socks5h://` but refuses
  `socks5://`, where curl resolves the name itself; so the service now takes `socks5://`
  as `socks5h://`. The project page carries the App Router's data, JSON-LD and Open
  Graph tags, and search (after its redirect) the App Router's data, with no Cloudflare
  check for the server's own address: the card and search can come from the pages
  without residential proxies for now.
- **Import (the adapter, `services/mods/src/curseforge.rs`).** A player gives a page
  address (`curseforge.com/minecraft/<class>/<slug>`, or `…/files/<id>` for one file);
  classes are mc-mods, modpacks, texture-packs, shaders, data-packs and worlds. A link
  straight to forgecdn fails with `curseforge_file_link`, since it names no project.
  The project's card (ID, name, summary, description, logo, authors, screenshots,
  categories, downloads, dates) comes from its page: the project object in the App
  Router stream, with JSON-LD and Open Graph for what it lacks. Its files come from the
  site's list, every page up to 20 pages of 50; CurseForge's files are our versions.
  As with Modrinth, the newest release of each game version and loader is fetched at
  once, with its dependencies and changelog; other files are listed and fetched when
  asked. The site gives no hashes, so a file is kept only when its size is the one the
  site lists, and its SHA-1 and SHA-512 are made here (the launcher's lookup finds it).
  Required dependencies are queued by their ID, class and slug, so one whose page fails
  still comes in, named by its slug until its page answers. Files marked early access
  or not released are left out. `lapi-mods probe` reads the JEI page with the same
  parser and prints which fields it found.
- **Third probe (2026-10-20, after the import went live):** every proxy answers now. The
  JEI page gives the project's ID, name, summary, logo, downloads, authors and
  categories, but not its description, screenshots or dates by the names first tried.
  The App Router stream gives large values by reference (`"$1f"`, a chunk elsewhere in
  the stream, text chunks as `1f:T<length>,…`) and dates as `"$D…"`; the reader now
  follows references, takes more names for dates and pictures, and the probe prints the
  shape of each such field (`fields[…]`, our words and lengths only).
- **Fourth probe (2026-10-20):** dates come as numbers (`dateCreated`, `updateDate`); the
  project page carries no description or screenshots at all, so CurseForge cards show
  the summary only for now.
- **A page's number is checked (2026-10-23).** RLCraft's page also lists other things
  with its slug (likely the servers that run it), and the reader took one of those for
  the project: its card got another number and no versions. The reader now takes, of
  the objects with the slug, the one most like the project (its class, Minecraft's
  game, the fields a card has); another class's or another game's object is never it.
  A number read from a page is used only when `/projects/<number>` leads back to that
  page; a project kept under a wrong number takes the right one at its next check.
  The probe reads RLCraft's page too and prints each candidate's number and likeness.
  Its first run showed the other object scoring above RLCraft's own (it names the
  modpack class, the project's object does not), so a page with several candidates
  has their numbers asked in turn, the project's current number first, then the
  likeliest, up to three; one candidate alone that is already the project's number
  is not asked. The probe now also prints, for each candidate, which fields it has,
  where its number leads and whether the site lists files for it.
- **The Installed tab's gaps (2026-10-24, D-072).** Each row says where its file came
  from before the path ("Modrinth · mods/sodium.jar"): the catalog's lookup and versions
  carry `source`, the lock keeps it at install, and files installed before that are
  looked up by their SHA-256 with the player's own. A file the catalog does not know
  shows the version its jar gives itself (its own mod, not one inside it) and no source.
  The tab reads the jars after the list (`mods_folder_check`, never holding the list up;
  only the last answer counts) and shows what may stop the set starting, as before a
  launch. Offers from a check stay through a file turned off or on, at its new path; a
  removed file takes its offer away, and an install or a pack update clears them.
- **Name search.** A name asks both sources: Modrinth's search API and CurseForge's
  search page (its API search is refused). The page's hits are read from the App Router
  stream (objects with a numeric ID, a slug, a name and a class we keep, Minecraft
  only), five at most, after Modrinth's. A CurseForge hit carries its import reference
  (`<id>/<class>/<slug>`) with the request; choosing it uses that stored reference,
  never one the player sends. One source failing leaves the other's hits. The probe
  prints how many hits it reads and whether JEI is among them.
- **Fifth probe (2026-10-20):** the search page carries no results; the site loads them
  in the browser from its search API, which refuses the server (403). The five "hits"
  read were the site's categories (an ID, slug, name and class too), so a hit now needs
  a summary and an author. Until the results can be had, a name finds CurseForge
  projects only by their page address.
- **Sixth probe (2026-10-20):** the search API refuses the server (403) also with a
  browser's fetch headers (Accept, Referer, Sec-Fetch-*), with the site's full set of
  parameters, and with both, directly and through every proxy. A name now asks Modrinth
  only; CurseForge projects come by their page address. The page reader stays for the
  probe.
- **Seventh probe (2026-10-20):** the project page does carry the description and the
  screenshots, apart from the project object: the description as a text chunk of the
  stream (JEI: 3.6 KB of HTML), the screenshots as objects with `url`, `thumbnailUrl`,
  `title`, `description` and `displayIndex`. The card reads both from there.

### Modpacks (stage 3)

- **Modrinth packs (`.mrpack`) first.** A pack's version is kept like any other (its
  archive). Its files are kept only when a launcher asks to install it
  (`POST /v1/versions/{id}/pack`, counted with the other fetches per address): the `pack`
  job reads `modrinth.index.json` from the kept archive and keeps every file it lists,
  checked against both hashes and the size. `GET /v1/versions/{id}/pack` gives the game
  version, the loader and its version, the files at our addresses, the archive (for its
  overrides) and the state: `none`, `working`, `ready`, or `failed` with a short code.
- **The index is checked:** format 1, game `minecraft`, one loader of fabric, quilt,
  forge or neoforge, versions as plain words, at most 5000 files and 16 GB together,
  SHA-1 and SHA-512 of each, a download from a host Modrinth allows (cdn.modrinth.com,
  github.com, raw.githubusercontent.com, gitlab.com; GitHub's redirects to its file
  hosts are followed). A path must be relative with `/` between names safe on every
  system, in a folder a pack fills (`mods`, `resourcepacks`, `shaderpacks`, `config`,
  `defaultconfigs`, `kubejs`, `scripts`, `saves`, `datapacks` and a few more mods use) or
  one of `options.txt`, `optionsof.txt`, `optionsshaders.txt`, `servers.dat`; no program
  (`.exe`, `.bat`, `.sh`, `.dll` and the like) anywhere, a `.jar` only in `mods/`. A file
  with any other path, or one the client does not use, is left out and counted.
- **In the launcher:** a Modrinth pack's version has **Install**. The dialog asks the
  catalog once to prepare the pack and then looks every 3 s, showing how many files it
  keeps; a failure says why in the player's words. Ready, it shows the game version,
  loader, mods and size (with the metered-connection notice, D-051) and asks for a name.
  The core then creates an installation with the pack's game version and loader (Forge's
  version as `<game>-<forge>`) and memory recommended for its mods (D-058), and puts the
  pack in: every file through the download executor, checked against its SHA-256; the
  archive the same way, then `overrides/` and `client-overrides/` (the later winning)
  unpacked by the same path rules, at most 20,000 files, 1 GB each and 4 GB together as
  unpacked, links never followed or made. The answer of the catalog is checked again
  (paths, hashes, addresses only at files-mods.lapi.sh, loader and versions) and a pack
  that does not fit is refused as a whole.
- **`content.lock.json` records the pack** (project, version, and the SHA-256 of every
  file and override it brought). Applying a pack again changes only what is still as the
  pack left it: a file the player changed stays, one turned off stays off, and what the
  previous version brought and this one no longer has is removed if unchanged. A pack
  that stopped part way can be finished with **Try again** on the same installation.
- **Updating a pack:** the Installed tab shows the pack an installation was made from and
  marks what it brought **From the modpack**; those files are updated with the pack, not
  one by one. **Check for updates** also looks for the newest version of the pack for the
  installation's Minecraft version. Updating to it backs up the whole installation first
  (reason "before a modpack update", D-047), then applies it as above; the loader follows
  the pack's version. A version for another Minecraft version or loader is refused: it
  installs as a new installation.
- **Saving an installation as a pack** (**Save as a modpack**) writes a `.mrpack` where
  the player chooses: the folders a pack fills (not `saves`, `local`, `journeymap` or
  `datapacks` at the root), worlds and `options*.txt` only when asked, never `servers.dat`,
  logs, turned-off files, links or programs, at most 20,000 files and 4 GB. Files in
  `mods`, `resourcepacks` and `shaderpacks` the catalog keeps from Modrinth (looked up by
  SHA-256, with an address on cdn.modrinth.com) are named in the index with their SHA-1,
  SHA-512 and size; everything else is copied into `overrides/`. Without the catalog,
  everything is copied. The archive is written beside the target and renamed into place.
- **CurseForge packs (stage 4, 2026-10-20).** A modpack version whose file is a `.zip`
  is read by its `manifest.json`: type `minecraftModpack`, version 1, the game version and
  its primary loader (`forge-`, `neoforge-`, `fabric-` or `quilt-` and a plain version),
  at most 5,000 files by project and file number, overrides only in `overrides/` (where
  the launcher looks). The manifest names no file, folder or hash, so the pack job asks
  the site for each file's name and size, keeps the file (or takes the copy already kept
  from the same address) and places it by what it is: a `.jar` in `mods/`, a zip with
  `shaders/` in `shaderpacks/`, one with `pack.mcmeta` and `assets/` in `resourcepacks/`.
  Anything else (a data pack, a name we may not write, a path another file has, a file
  the site no longer has) is left out and counted; `required: false` makes a file optional. Until found, a file is
  counted in `total` but not listed. From there it is the same list as a `.mrpack`'s,
  and the launcher installs and updates both alike.
- **One project from both sources (stage 4, 2026-10-20).** When a kept file of a version
  has the SHA-256 or SHA-1 of a file of another project of the same kind from the other
  source, the two become one (`join_twins`): the Modrinth one stays (else the older), the
  other's sources, versions, categories and requests move to it, and its ID and slug lead
  to it (`project_joined`, status `joined`). Two projects of one source are never joined.
  When the other had the plain slug first (the staying one then got
  `<slug>-<source>-<id end>`), the staying one takes the plain slug and the long one
  goes to the joined project, so both still lead to it; the service also fixes joins
  made before this at start (2026-10-21: Fabric API had become
  `fabric-api-modrinth-…`).
- **The catalog keeps the popular projects and checks itself (2026-10-21, Ai: "сделай
  всё сразу").** Once a day the service queues Modrinth's 100 most downloaded projects
  of each kind it does not have yet (`LAPI_MODS_POPULAR`; 50 until Ai asked for 50 more
  on 2026-10-22); CurseForge's search answers the server with 403 and Modrinth has no
  worlds, so worlds still come by address. Lists by
  downloads (no words) put libraries (`library`, `library-api`, `api-and-library`,
  `api`) after the other projects; a search for words keeps its relevance. A mod that
  also publishes data pack versions (Terralith) is found among data packs too, and the
  launcher puts such a version into a world as a data pack; a project's file in one
  folder (a world, `mods`) no longer replaces its file in another. `lapi-mods slug`
  renames a project on the server without a deploy, and `lapi-mods health` runs daily
  in the workflow Mods health, failing on a problem (docs/MODS-CATALOG.md).
- **Data packs from Modrinth (2026-10-21).** Modrinth's API calls a data pack a mod
  (`project_type` `mod`) and tells it only by its loaders. A project whose only loader is
  `datapack` is a data pack in the catalog; one that is also a mod for a loader stays a
  mod, since it installs as one.
  The card (title, description, pictures) is Modrinth's; reading CurseForge again only
  adds its versions. A file on both sources shows once in the versions list, as
  Modrinth's. The launcher follows a join in its lock: the lock's files are looked up by
  SHA-256 before a plan, an update check or a removal warning, and a file whose version
  now belongs to another project takes that project's ID, so a mod installed from
  CurseForge is still known as there.
- **Maps (stage 4, 2026-10-20).** A map (a project of kind `world`, CurseForge's worlds)
  is added to an installation as a new world (`mods_world_plan`, `mods_install_world`,
  `mods/world.rs`). Its version's file must be a `.zip` of at most 8 GiB; the world is the
  folder with `level.dat` at the top of the zip or one folder down (the shallowest;
  `__MACOSX` is skipped). Each entry is checked like a pack's overrides: names safe on
  every system, no links, at most 32 folders deep, 200 000 entries and 16 GiB unpacked.
  It is unpacked into a hidden folder in `saves/` and renamed into place under the map's
  title, or `title (2)`, `(3)`… when a world has that name, so no world of the player's
  is ever overwritten. The zip is removed after. A map is not recorded in the content
  lock: once played, it is the player's world.
- **From other launchers and folders (stage 5, 2026-10-20).** "Bring in from another
  launcher" on the Installations screen makes a new installation from a Prism Launcher or
  MultiMC instance (its folder, or the zip it exports), a game folder with `mods/` (at the
  top, in `.minecraft/` or `minecraft/`, or one folder down), or a folder of `.jar` files
  (they go into `mods/`). The player picks it in the native dialog; the core keeps the
  pick (`mods/import.rs`, `mods_import_pick`), so the window never names a path, and
  copies only into the installation made for it (`mods_import_create`,
  `mods_import_apply`). Only what a pack may write is copied (`pack_path`), links are not
  followed, at most 200 000 entries, 50 000 files and 32 GiB. The game version and loader
  come from `mmc-pack.json` (Forge's `47.2.0` is matched to `1.20.1-47.2.0` in the loader's
  list); for a folder they are guessed from most mods' `fabric.mod.json`,
  `quilt.mod.json` (one Quilt mod makes it Quilt), `META-INF/mods.toml`,
  `META-INF/neoforge.mods.toml` or `mcmod.info`, and the player can change both. The
  loader's version is the one the source names, else the recommended, else the newest
  stable. A copy that stopped finishes on a retry, keeping files already in place. The
  catalog knows the copied mods by hash as it does any file of the player's.
- **packwiz packs (stage 5, 2026-10-20).** "Add to the catalog" takes the address of a
  packwiz `pack.toml` on GitHub (a file's page becomes its raw address), GitLab or
  raw.githubusercontent.com. The catalog (`services/mods/src/packwiz.rs`) reads it, checks
  its `index.toml` against the hash `pack.toml` gives and every `.pw.toml` and file
  against the index, takes each mod from its URL (a host a modpack may download from) or,
  for `metadata:curseforge`, from CurseForge by project and file (an md5 or murmur2 hash
  only there), leaves out server-only mods and paths a pack may not write, keeps every
  file and writes the pack out as a `.mrpack` (mods in the index, other files under
  `overrides/`, at most 64 MiB each and 512 MiB in all). It becomes a version of a
  modpack project of source `packwiz` (the pack read again unchanged is the same
  version; a changed pack is a new one), so the launcher installs and updates it as any
  modpack. The weekly re-check reads the pack again.

### Upkeep (stage 6)

- **Owner's choices (2026-10-20):** quarantine only for a combination of strong signs or
  a known marker; files served from Cloudflare R2 with our server as the reserve; a
  Cloudflare token for purging the cache; players' own packs published at once; the
  owner's commands over SSH and as workflow tasks that print numbers and hashes only.
- **The scanner (`services/mods/src/scan.rs`).** Every jar or zip the catalog keeps is
  read before it is served: each class file for the methods it calls, the strings of
  its constant pool and the strings its code builds byte by byte (how fractureiser hid
  its address), and nested jars three levels deep (at most 100 000 entries, 8 MiB a
  class, 64 MiB a nested jar, 1 GiB read in all). Weak signs (the network, starting
  programs, loading code, programs inside, files where sign-ins live, autostart places)
  are only recorded. A file is held back for strong signs together: loading code with
  the network or a hidden address in one class, a hidden address with starting a
  program, sign-in files with the network, autostart with programs or the network; or
  for a marker of a known family (fractureiser's addresses), which makes it malware.
  Checked on the 61 most downloaded mods on Modrinth (2026-10-20): 30 clean, 30 noted,
  one held (Essential: its own account files and downloaded code), for the owner to keep.
- **Held files** move to `data/quarantine/` (0600, nginx does not look there). The API
  gives their versions' and packs' files no address and `held: true`; a pack with one
  fails with `held`. Suspected files go before the owner (`review`, kind `scan`); a
  version its source takes down goes there too (kind `gone`) and stays offered. Files
  kept before the scanner, and all of them whenever its rules change (`scan::RULES`), are
  scanned again in the background, 20 every 30 seconds.
- **Warning players.** `GET /v1/harmful?since=&after=` lists, page by page, the files
  held back as malware (not those only suspected). The launcher reads it at most once an
  hour and the Installed tab warns about such a file in an installation, by SHA-1, with
  Remove. A held file is not installed: the plan names it, a pack with one fails.
- `lapi-mods scan <file>` prints the verdict and signs of a file; `counts` adds held,
  noted, waiting for a scan and open reviews.
- **The owner's commands (`services/mods/src/owner.rs`)**, in the same binary, over SSH:
  `find` (a project by address, slug or ID, or a file by SHA-256 or SHA-1: state,
  verdict, signs, where it is used, decisions), `review` (the queue) and
  `review <n> keep|block`, `block <hash> [reason]`, `unblock <hash>`,
  `remove <project> [reason]` and `forget <sha256> --yes`. A block is kept by hash
  (table `blocked`), also by a SHA-1 from a public report before the file is here: the
  file leaves service at once, a copy fetched again goes straight into quarantine, and
  launchers find it in `/v1/harmful`. `unblock` (and `review keep`) puts a held file
  back and marks it allowed, so no later scan holds it. A block clears the file's
  addresses from Cloudflare's cache (they are cached for a year) with a token that may
  only purge the zone (`LAPI_MODS_CF_ZONE`, `LAPI_MODS_CF_TOKEN`; curl reads the token
  on standard input). `remove` hides a project for good (an import fails with
  `removed`) and holds back the files no other project needs; `forget` deletes a held
  file, and the catalog does not fetch it again. Every decision goes into the table
  `decision` with its time and reason. The workflow runs `block`, `unblock` and
  `review --count` (task `review-count`) behind the production approval; they print
  fixed words, numbers and the hash given only.
- **Serving from Cloudflare R2 (`services/mods/src/r2.rs`, `upkeep.rs`).** A public
  bucket (`mods-files`, key `f/<sha256>`) behind `https://cdn-mods.lapi.sh/f/<sha256>`,
  and a private one (`mods-private`) with copies of held files (`q/<sha256>`) and of the
  database (`db/daily/<day>.db` for a month, `db/monthly/<month>.db` for good). R2 does not
  charge for what it serves, so players' downloads cost storage only. The service copies
  kept files in the background, 20 at a time, and the column `blob.copy` records where
  each one is; an upload names the file's SHA-256 and R2 refuses other bytes. The API
  gives the R2 address only for a file with a public copy and once `LAPI_MODS_CDN_URL`
  is set; any other file, and every picture, is served from our disk as before, so the
  move is gradual and old addresses keep working. A file held back, blocked or removed
  leaves the public bucket (and Cloudflare's cache) in the next round; a held one gets
  a private copy, a forgotten one loses it. Once a week 50 public copies are checked
  against the disk and a wrong one is copied again; `counts` shows how many files are
  in R2, still to copy, and when the database was last copied. Our disk stays the
  reserve: nginx also serves `files-mods.lapi.sh/f/<sha256>`, and the launcher, when
  cdn-mods does not answer or lacks a file, takes the same path from files-mods,
  whatever the mirror setting. `lapi-mods restore` brings back to the disk, checked by
  SHA-256, what only R2 has. R2's access key reaches curl on standard input
  (`--aws-sigv4`), never on a command line.
- **Players' own packs (`services/mods/src/own.rs`, launcher `src-tauri/src/mods/publish.rs`).**
  The Installed tab's _Publish to the catalog_ sorts the installation's files first:
  mods, resource packs and shaders are named by their hashes and must be in the catalog
  (a mod it lacks is named, to be added first); settings and configs go into
  `overrides/` by the catalog's rules (never under `mods/`, no code or program under
  any extension, 64 MiB a file, 512 MiB in all); turned-off files, logs and the server
  list stay out, worlds and `options.txt` only when the player asks. The launcher writes
  an `.mrpack` with `catalog.json` (title, summary, description in Markdown, version
  number, changes) and the icon the player chose (PNG, JPEG or WebP, 512 KB and 2048
  pixels at most), and sends it with the launcher account's token in parts, since
  Cloudflare takes requests up to 100 MB: `POST /v1/packs/uploads[?project=<id>]` with
  its size and SHA-256 (the account's limits are checked here), then each part of
  64 MiB to `PUT /v1/packs/uploads/<upload>/<n>` (in any order, a part sent again
  replaces the first; a part that breaks off is sent again twice at most), then
  `POST /v1/packs/uploads/<upload>/finish`, which puts them together and checks the
  size and SHA-256. Uploads live in memory and `tmp/`: one per account, eight on the
  server, 30 idle minutes, gone on a restart (the launcher says to publish again).
  `POST /v1/packs[?project=<id>]` takes an archive of up to 90 MiB in one request. The
  catalog checks it all again: index files are
  found among kept files by SHA-1 and SHA-512 (the addresses the index names are not
  used), none held or blocked; the texts (`<` is escaped, the Markdown renders through
  the same safe tree as other descriptions); the icon decoded with limits; and the
  scanner over the archive, whose hold refuses the pack. The pack is then a project of
  kind `modpack` with the source `own`, its author the account's nick, shown at once and
  installed like any other pack; it is never refreshed from a source. Its slug starts
  with `player-` (a pack called "Sodium" is `player-sodium`), so a player's pack never
  takes the name of a project a catalog brings in later, and the launcher marks its page
  _Player's pack_; the prefix is left out of the search index. Packs published before
  the prefix got it at the service's next start. Publishing (a pack, a room pack, or
  any part of one) needs a confirmed email once the account service sends letters
  (`LAPI_MODS_REQUIRE_EMAIL`: `auto`, the default, or `1` always, `0` never), as
  friends does; the catalog asks the account service every 15 seconds and refuses with
  `403 email_not_confirmed`, which the launcher shows. The archive is
  512 MiB at most (nginx allows 91 MiB on `/v1/packs` and 65 MiB a part on
  `/v1/packs/uploads/`). An account publishes 5 new packs and 20
  versions a day, 2 GB in all, 10 an hour (20 an hour from one address). The
  installation remembers the pack (`content.lock.json`, `published`), so the next
  publish is a new version with a higher number (a used number answers 409); the author
  hides the pack or shows it again (`POST /v1/packs/<id>/hidden`) and lists theirs
  (`GET /v1/packs/mine`). A pack the owner removed takes no versions (410). A player
  reports another player's pack (`POST /v1/projects/<id>/report`: malware, stolen,
  inappropriate, broken or other, 500 characters; 10 an hour); the reports go before the
  owner (`review`, kind `report`), where `keep` dismisses them and `block` removes the
  pack. Projects from Modrinth and CurseForge are reported at their source.

### Projects from other sites (external source)

- **Only the owner adds them** (Ai, 2026-10-21): commands `lapi-mods external …`, no
  "owner" screen in the launcher and nothing for players. Their files are kept here like
  any other (the authors allowed it, Ai, 2026-10-21) and scanned the same way; whatever
  comes back must be a zip archive (a jar, `.zip` or `.mrpack`) of at least 1 KB, so a
  site's error page is never kept. A file has no hashes from its site: ours are made when
  it is kept, and a file already kept is never fetched again.
- **OptiFine** (`optifine`, source `external`): read from optifine.net's downloads page,
  a version per row under its `<h2>Minecraft …</h2>` (a file naming another game version
  is left out; `1.8.0` is the catalog's `1.8`). Rows that name a Forge build are mods for
  Forge (`forge`), the build written in the version's changes; previews are `beta`; rows
  without Forge carry no loader (OptiFine on its own is a loader of its own, D-039). As on
  Modrinth, the newest for each game version and loader is kept at once, the rest on
  request; the changelog is read once, for new versions only. A file comes through the
  page its link leads to (`adloadx?f=<file>`), whose link must be
  `downloadx?f=<the same file>&x=<32 hex>` on optifine.net.
- **By hand** (`manual:<slug>`): a card the owner writes (title, kind, page, summary,
  icon) and versions one file each, given by its address on any public HTTPS site
  (never an address or an inner name), with game versions, loaders, number and channel.
- **Checked every N days, each its own** (Ai, 2026-10-21): `recheck_days` on the project,
  1 to 365 (0: never; a check by hand still runs). External projects use it as their
  time (OptiFine 3 days, others 7 unless set); for a Modrinth or CurseForge project it is
  the longest time between checks (`lapi-mods external days <project> <N|default>`).
- **Trust, each project its own** (Ai, 2026-10-21): the scanner suspects OptiFine's
  jars (its installer loads code, reaches the network and runs programs by design), so
  every new build waited for an `unblock`. `lapi-mods external trust <project> on`
  (`external_project.trusted`) lets go its held files the scanner only suspects, and from
  then on such a file of its own versions (source `external`) is served when it is kept
  or rescanned, as if the owner unblocked it (`allowed`, review closed, `trust` in the
  journal). A known malware marker and the owner's block hold it all the same. `off`
  stops it for new files; files let go already stay served (`block` takes one back).
- **VirusTotal for suspected files** (Ai, 2026-10-21): with the secret
  `VIRUSTOTAL_API_KEY` (`LAPI_MODS_VIRUSTOTAL_KEY` on the server) the service asks
  VirusTotal about each file the scanner suspects (held, or let go by `unblock` or
  trust), newest first, one every 15 seconds while any waits and the day's quota lasts
  (Ai), otherwise looking again each minute. A file VirusTotal does not know is sent to
  it when it comes from Modrinth, CurseForge or the owner's projects (up to 650 MB;
  files only players' own packs carry are never sent) and looked up again after 30
  minutes, 2 hours, then daily. Each file is asked about once, no re-checks for now
  (Ai); `lapi-mods virustotal check <hash>` asks again. Quotas, counted here for every
  request by UTC day: 500 a day and 15 500 a month, of which the background takes at
  most 450 and 15 000. The answer is shown only (`virustotal: 2 / 60` in `find` and
  `review`, `virustotal status`): it holds back or lets go nothing; engine and threat
  names are not kept.
- **Sites the owner names are paced** at 20 requests a minute an exit, rested after a
  refusal like the others; optifine.net is a listed host.
- **The owner adds catalog projects** (Ai, 2026-10-21): `lapi-mods add <address>...`
  (Actions `task=add`) adds up to 32 Modrinth or CurseForge projects by their address,
  as a player's "Add to the catalog" does, under the account `owner`; names are refused,
  since a search asks someone to choose. `add --status` counts them by state.
  OptiFine's icon is one made for the catalog (`services/mods/assets`), since its site
  has none.
- **A set of mods is checked by what the jars say (2026-10-22, Ai: "Всё сразу").**
  Pages say only part of what a mod needs, and sometimes for the wrong game version, so
  the shared crate `crates/mod-meta` reads a jar as the loaders do (`fabric.mod.json`,
  `quilt.mod.json`, `mods.toml`, `neoforge.mods.toml`, `mcmod.info`, jars inside jars)
  and checks a set against the game version, the loader and its version. The catalog
  reads every jar it keeps and gives it with a version (`files[].manifests`) and by mod
  ID (`/v1/provides/{mod}`). An install plan takes for a dependency the newest version
  whose jar runs on the installation's loader version (it is fixed with the
  installation), looks a mod only a jar names up by its ID, updates a mod there already
  that is too old for a new one, and offers another version of the mod asked for when
  that one does not run. Before a launch the folder's jars are checked on the computer.
  Mods that say nothing of each other but do not start together (OptiFine with Sodium,
  Iris, Embeddium, Rubidium, Oculus) are a list the owner keeps in the catalog
  (`/v1/clashes`, `clash add|remove`); the launcher has it built in for when the
  catalog is not reached, and a launch waits for it at most three seconds. What is left
  is a warning ("start anyway", "open the mods", "not now"), never a block, and what
  cannot be compared counts as fine, so a set that starts is never stopped.
- **Browsing for concept A (2026-10-22, D-072).** The tiles, collections and categories
  the owner chose need the catalog to say more. Categories are one shared list
  (`services/mods/src/categories.rs`, IDs like `mod.optimization`), each mapping
  Modrinth's and CurseForge's own slugs for one kind; a search takes up to 8 at once and
  `/v1/categories` counts the projects in each for the same search. Hits carry their
  loaders, shared categories and a cover (the gallery's featured picture). Collections
  are the week's most popular (downloads gained, from a daily copy of the sources'
  public numbers: our own installs are counted only for players who chose to send
  statistics), what was added in 7 days, players' packs, and the owner's own, titled
  in ru, en and ar (`lapi-mods collection`). A version's files say whether the
  catalog's check found them clean (`checked`); dependencies carry their title, icon
  and slug; versions come a page at a time with the game versions and loaders to filter
  by; `/similar` gives up to five projects of the same kind sharing most categories.
- **Installed mods and the crash window (2026-10-22, D-072).** The search finds a mod,
  pack or shader in every installation by the words of its title or file name, or by
  its catalog project (`mods_find_installed`, at most 100, the last played installation
  first), so it can say where a mod is installed. When the game stopped because a mod is
  missing or too old (`missing_dependency`, `wrong_dependency_version`), the crash
  window asks the catalog for the mod's project by its ID and the newest release whose
  jar gives the version the jars there need (`mods_fix`), and offers "Update <mod>" or
  "Install <mod>" with the usual plan. Offline, or when the catalog has nothing
  better, the button is not shown.
- **A modpack's mods are catalog projects (2026-10-23).** A modpack's files were only
  blobs: kept once, but its mods had no page, title or icon unless someone added them.
  The catalog now adds the project of each mod, resource pack and shader of the
  modpacks it keeps, from the file's Modrinth address or by asking Modrinth for its
  SHA-1, a few at a time while the queue is quiet. A mod only on CurseForge is added
  by its number: CurseForge's site leads `/projects/<number>` to the project's page
  (checked from the server on every exit), so it is still reached by link only, with
  no API and no search. The catalog's lookup by hash tells the title, icon and version number, and
  the launcher asks it each time it lists an installation, so the Installed tab names
  a modpack's files (and the player's own) without "Check for updates".

## D-067 — Start page feed at feed.lapi.sh

- **Status:** accepted (owner request, 2026-10-19; design approved in the project) ·
  **Affects:** the launcher's tabs (a new Feed tab, the "More" menu, the tab opened at
  launch), the new service `services/feed` (`https://feed.lapi.sh`,
  `/home/www/lapi/feed`), a player window for videos and streams.
- **Goal.** The launcher opens on a Minecraft feed: patch notes of new versions, Mojang's
  Java news, YouTube videos about Minecraft, live Twitch streams and our own events and
  tips. The launcher asks no one but our server for it: no minecraft.net, YouTube or
  Twitch request from a player's computer until they choose to watch.
- **Sources (the service only):** Mojang's launcher content (`launchercontent.mojang.com`
  v2: Java patch notes, news) and the version manifest; YouTube channel RSS feeds (no API
  key), a channel found by its link, @handle or a video (oEmbed); Twitch Helix with an
  app token (`TWITCH_CLIENT_ID`, `TWITCH_CLIENT_SECRET`), game Minecraft, mature streams
  left out; DeepL (`DEEPL_API_KEY`) translates patch notes and news into Russian and
  Arabic, stopping at 90 % of the month's characters.
- **No HTML reaches the launcher.** Articles are cleaned into our own blocks (headings,
  paragraphs, lists, pictures; runs of bold, italic, code and https links); scripts,
  frames, forms, attributes and every other address are dropped, nesting and size are
  bounded. Translations go out as that HTML and come back through the same cleaning.
- **Pictures** are fetched by the service, decoded with limits and re-encoded as JPEG:
  article pictures by their SHA-256, video previews only of videos of channels the
  service knows, stream previews only of streams in the feed, so no one makes it fetch
  arbitrary addresses.
- **Editorial** (`services/feed/editorial.toml`, built in): the channels per language
  (approved list), blocked Twitch logins, events, announcements and daily tips in en, ru
  and ar; links only to YouTube, Twitch, minecraft.net and our domain.
- **API:** `GET /v1/feed?lang=` (a minute), `GET /v1/notes/{id}?lang=`, pictures
  `/v1/img|yt|tw/…`, `POST /v1/channels/resolve` (a player's own channel; 10 a minute per
  address), `GET /v1/videos?channels=` (up to 30 of a player's channels, kept on the
  player's computer only), `/play/yt/{id}` and `/play/twitch/{login}`: a page with the
  provider's embedded player under a CSP with no script of ours. Nothing about a player
  is logged; nginx logs paths without query strings.
- **In the launcher:** the Feed tab; the tab opened at launch is a setting (feed by
  default; home when a game runs); Backups, Screenshots, Diagnostics, Console and About
  move into "More". Videos and streams play in a player window (default) or the
  browser, as the player chooses.
- **Server setup (owner):** a DNS record for feed.lapi.sh behind Cloudflare, then
  `deploy/bootstrap.sh` once, as for the other services.
- **Endless list (owner request, 2026-10-20):** under the newest version, the feed is one
  list of patch notes, news, videos and streams mixed, newest first, one block below the
  other, and the next page loads as the end comes near. `GET /v1/page` (a minute; notes,
  videos, kinds, snapshots, shorts, before, limit) pages through it; `before` is a
  cursor the service made (`<time>~<key>`), checked on both sides, at most 40 items a
  page. The service keeps up to 400 patch notes and 300 news (20 new bodies a refresh)
  and 200 videos per channel (its RSS lists 15; older ones stay from earlier fetches);
  `/v1/feed` still holds the newest 40. Pages are not kept on the player's computer:
  without the service the list is the feed kept on disk.
- **A varied, live list (owner request, 2026-10-21):** the service spreads videos so no
  channel has more than 2 of any 10 in a row (a moved video takes a time just below the
  one before it, so the cursor still holds), and pages are shown in the service's order.
  Up to 24 streams per language, Helix asked every 2 minutes: the mixed list and the
  whole feed show the 6 most watched, the Streams chip pages through all of them. While
  the launcher is shown it asks for live streams every 3 minutes: ended ones leave the
  list, and under the Streams chip new ones come in. Channels can
  be put first or hidden from a tile's menu (kept in `feed-channels.json`). Videos played
  from the feed are kept in `feed-watched.json` (the latest 500 ids) and shown dimmed. A
  system notification when a favourite Twitch channel goes live is a setting, off by
  default: the core looks every 3 minutes, tells each start once, and only while the
  launcher is not in front.
- **Pictures for the mosaic (D-072, 2026-10-22):** a Short also has an upright preview
  `ys/<id>.jpg` (`imageTall`, up to 540×960: YouTube's own upright picture, else the
  middle of its 16:9 preview); videos and streams have the channel's picture `ya/<channel id>.jpg`
  or `ta/<login>.jpg` (`avatar`, 96 px square). The service looks a channel's picture up
  only when a launcher asks for it, and only for channels it knows (ours, those players
  named, streams it lists): from the channel page on YouTube, from Helix on Twitch, at
  most once in 10 minutes each, kept a week. Note cards carry their picture's
  `imageWidth` and `imageHeight`. Since 2026-10-23 video previews are made from the
  largest picture YouTube has (1280×720, else 640×480 or 480×360 without the black bars,
  320×180 last) at 960×540, stream previews at 960×540, JPEG quality 85 with Lanczos
  scaling; previews stored narrower than 400 px are made again once a day.
- **Video lengths (owner request, 2026-10-22):** RSS has none, so the service asks
  YouTube's Data API v3 with a key (secret `YOUTUBE_API_KEY`) for videos that have
  none yet: 1 unit per 50 videos, kept with the video, so a day takes a few hundred of
  the free 10 000 units; it stops at 8 000 a day. Cards carry `duration` in seconds when
  known (not for live streams or premieres). With the key, channel pictures come from
  the API too (1 unit) instead of the 1.4 MB channel page. Without it both work as before.
- **Kept between screens (owner request, 2026-10-23):** the feed and each chip's list stay
  in memory for the run (never on disk), so going back shows the same cards at once,
  scrolled as they were. A list shown again within a minute is not asked again; after
  that, every 3 minutes while it is shown and on Refresh, its first page is looked at
  again: new cards come in at the top (each before the next card the list has), drop in
  one after another with a mint glow while the others slide to their new places, and
  cards it has are updated where they are. Further down the list new cards wait behind
  a "N new posts above" pill until the player is back at the top. Refresh never builds
  the list anew; a note says what it found, "nothing new" included. Motion stops under
  `prefers-reduced-motion`.

## D-068 — Worlds with Friends

- **Status:** accepted (owner request and design approval, 2026-10-19) · **Affects:** the
  launcher's core (a Minecraft server beside one game, a tunnel), the activity permit
  (`state/activity.rs`), world launches without `--demo` for verified owners, docs/spec/01
  (server hosting and one game beside one server are added for worlds), the new services
  `services/worlds` (`https://worlds.lapi.sh`, `/home/www/lapi/worlds`) and a relay at
  `relay.lapi.sh` on a separate VPS.
- **Goal.** Create a private world, invite friends and play together from different
  networks without router setup: the launcher runs the world's server on the host's
  computer, and friends connect through an encrypted tunnel, directly or through our relay.
- **Specification.** The owner's Worlds with Friends specification 1.0.2 is kept unchanged in
  `docs/worlds-with-friends/spec/`; how this launcher implements it is in
  `docs/worlds-with-friends/ADAPTATION.md`, which wins where they differ and is updated as
  work goes on. Phases are W1–W12 (`PLAN.md`), state in `PROGRESS.md`.
- **Owner's choices:**
  - The license is checked once in the launcher, as today: a Microsoft account with a
    Minecraft entitlement signed in on this computer. In a world the game runs under the
    launcher account's nick without `--demo` only then; players without a license keep the
    demo and cannot open worlds. Outside worlds D-033, D-045 and D-064 are unchanged.
  - Room servers run `online-mode=false`, so play does not depend on reaching Mojang. Rooms
    are isolated by the tunnel: the server listens on 127.0.0.1 only, a single-use 60-second
    ticket per member, room, session and device, the host checks the nick in the login packet
    against the ticket before the server sees a byte, and a whitelist of the members' offline
    UUIDs is the second layer. A changed nick moves the player's saved files to the new UUID.
  - The relay runs on its own VPS so the main server's address stays behind Cloudflare.
- **Other defaults:** the host lives in the launcher's core (no separate agent until a home
  host without the launcher); worlds.lapi.sh on axum and SQLite like the other services, port
  8788, oauth audience `worlds`, DPoP with an Ed25519 device key; iroh for the tunnel, pinned
  after a spike; first world version 1.21.1 without a loader on Java 21; up to 8 players; one
  room server and one game per computer; invitations by a card in the friends chat or a code
  (24 h, one use by default, 7 days at most).
- **Known limit:** the license check runs in the launcher, so a modified build could skip it;
  the owner accepted this.
- **Server setup (owner):** DNS and certificate for worlds.lapi.sh and `deploy/bootstrap.sh`
  once (W5); a VPS, an unproxied DNS record for relay.lapi.sh and `RELAY_SSH_*` secrets (W6).

## D-069 — Ads for Minecraft servers at servers.lapi.sh

- **Status:** accepted (owner request and design approval, 2026-10-21; design in the
  project) · **Affects:** the new service `services/servers` (`https://servers.lapi.sh`,
  `/home/www/lapi/servers`, port 8789), the Feed tab (an ad tile), launches straight into a
  server.
- **Goal.** Show ads for Minecraft servers in the launcher, first in the feed: the server's
  icon, the ad's text and the address, a Play button that installs the version if it is
  missing and starts the game straight into the server, and a button to pick another of the
  versions the server lets in.
- **An ad** has a name, an address (as the launcher reads them, `launch/server.rs`), a
  default version and a range of releases from Mojang's manifest (no snapshots), a text of
  up to 200 characters (no links, no control or layout characters; optional texts in en, ru
  and ar), a start and a run from 1 minute to 5 years, a weight among the ads showing at
  once, a place (`feed` for now) and an optional erid (the Russian ad register's token).
- **Read only from outside.** The service answers `GET /v1/ads?place=&lang=` (30 seconds),
  `GET /v1/icons/{sha256}.png` (for good) and `/v1/health`; it has no route that takes a
  body, and nginx lets through GET and HEAD of those paths only. It listens on 127.0.0.1.
  Ads are written by the owner's commands on the server alone (`lapi-servers add|edit|…`
  as the service's user, `docs/SERVER-ADS.md`), never through a workflow, whose logs are
  public; the one exception is the test ad the owner asked for (task `test-ad`), of a public
  server, which prints fixed words and a number.
- **Icons and players online** come from a status ping (Server List Ping) by the service,
  every hour and when an ad is added: a player's launcher asks no server until its player
  presses Play. Every length in the ping is bounded by our own code (the ping library looked
  at, craftping 0.8, reads whatever length a server names); SRV records are followed with
  hickory-resolver, and only public addresses are pinged, before and after DNS. Icons are
  decoded with limits, made 64×64 and written again as PNG.
- **No statistics** in this first version: counting views would mean taking writes from
  outside.
- **Server setup (owner):** a DNS record for servers.lapi.sh behind Cloudflare, then
  `deploy/bootstrap.sh` once.
- **Amendment (owner, 2026-10-21): languages, banners, licensed servers.** An ad may be
  shown only to players whose launcher is in some languages (`--langs ru,en`; the language
  the launcher asks with). The owner may give a 16:9 banner (`banner <id> <file>`, PNG or
  JPEG, cut to 16:9, made 640×360 and written again as JPEG), served at
  `/v1/icons/{sha256}.jpg` beside the icons, so nginx needs no new path. Once a day, and on
  `add`, `edit` and `ping`, the service starts a login as `ServerListCheck` in the protocol
  the server's status named and reads the first answer only: an encryption request means
  the server wants a Microsoft sign-in (online mode); login success or compression means it
  does not; anything else is unknown. The launcher says "licensed accounts only" on such an
  ad, and Play tells a player who would play without a Microsoft account so before anything
  is downloaded.
- **Amendment (2026-10-21): the check's protocol.** A server that takes many versions
  repeats in its status the protocol it was asked in and may still refuse it at login: the
  first real ad (asked in 1.21.1's protocol) kicked 1.21.6 and older for their version and
  asked 1.21.7 to 1.21.11 for encryption, and dropped connections that came a second apart.
  The check now logs in first in the protocol of the ad's default version (a table of
  release protocols in `ping.rs`; a release missing there is not guessed), then in the one
  the status named, then in the newest known, at most three, each after a kick or a failed
  connection and each four seconds after the last connection.

## D-070 — Reviews from players at reviews.lapi.sh

- **Status:** accepted (owner request and design approval, 2026-10-21; design in the
  project) · **Affects:** the new service `services/reviews` (`https://reviews.lapi.sh`,
  `/home/www/lapi/reviews`, port 8790), the More menu ("Leave a review"), the account
  service's tokens (audience `reviews`).
- **Goal.** Players tell the developers about a bug, an idea, a question or their
  impression from inside the launcher, with screenshots, text files and, if they want, the
  latest logs found and scrubbed by the launcher; the owner reads and answers on the server,
  and the answer shows in the launcher.
- **A review** has a kind (`bug`, `idea`, `question`, `impression`), a category and
  subcategory from `services/reviews/categories.json` (one file for the service and the
  launcher; an unknown subcategory is kept as `other`), a message of 20–5000 characters, an
  optional rating 1–5 (impressions only), the launcher's version, OS and language, the
  install ID, and optionally the diagnostic ID of a launcher error or the number of a server
  ad complained about. Files: up to 5 pictures (PNG, JPEG or WebP by content; the launcher
  scales them to 2560 pixels; the service decodes them within 8192 pixels a side and 40
  million in all and encodes them again, so no metadata stays) and up to 12 text files of
  512 KiB (logs, configs: a list of extensions, valid UTF-8, no NUL, almost no control
  characters), 16 MiB a request.
- **Logs.** "Attach the latest logs" collects the launcher log's tail, the last run of the
  chosen installation (7 days at most), the launcher's failure records of 3 days and a
  `system.txt`; everything goes through the crash reports' scrubbing (D-061) and the player
  sees every file before sending. Coming from the crash panel, that run's files are taken;
  its `diagnosis.json` names the crash's signature, which `lapi-crash` groups by.
- **Answers.** The service returns a number and a random key (it keeps only its SHA-256).
  The launcher keeps both and asks `POST /v1/status` for the answers (keys in the body, not
  in an address), at most every 6 hours and only while a review is open. A player who links
  the review to their launcher account is known by the account ID only (the token is
  checked with the account service's published key and never kept), and sees the answers
  on any computer. Answers are plain text. A player can delete their review.
- **Spam.** Before a review the launcher solves a proof of work (`GET /v1/challenge`, a
  signed challenge for ten minutes, taken once; 20 bits, about a second; a restart makes
  the challenges issued before it void). Limits run before a body is read: per address
  (an IPv6 /64 counts as one) 5 an hour and 15 a day, five times that per IPv6 /48, per
  install and per account 10 a day, everyone together 300 an hour and 1000 a day, two
  bodies decoded at once, 10 GiB of files. Many links, hardly any letters or the same text
  from one network within a day marks a review as suspect instead of refusing it. Bans by
  install, account or network; a network is kept only as an HMAC of the address (or its
  /64) with the service's secret, for 30 days. A banned sender is answered as anyone is and
  nothing is kept. `pause` stops taking reviews; the launcher keeps them in its outbox.
- **Privacy.** Kept 180 days (files 90, suspect 14). No address is stored or logged (nginx
  writes its anonymous format); the service's log has fixed words and numbers only. Reviews
  are read only on the server with `lapi-reviews` (docs/REVIEWS.md); the workflow's
  `task=counts` prints numbers. `show --translate` sends a message to DeepL only when the
  owner asks.
- **In the launcher.** More → Leave a review, with a dot on More while an answer is
  unread. "Tell us more" on the crash panel and on the launcher's failure offer, and
  "Report" on a server ad, open the form with the category, the run, the failure or the ad
  filled in. A review the service cannot take now waits in `reviews/outbox` of the data
  folder (30 days) and is sent again in the background; `reviews/sent.json` keeps the
  numbers and keys. Pictures from a file or the clipboard are drawn again in the window,
  screenshots from the gallery are scaled by the core; neither keeps metadata. Files are dropped on a field in the form: the main window
  turns the native drop handler off (`dragDropEnabled: false`), so the page gets the files'
  contents and never their paths, and a file dropped elsewhere is ignored.
- **Server setup (owner):** a DNS record for reviews.lapi.sh behind Cloudflare, then
  `deploy/bootstrap.sh` once, after the first deploy has uploaded the new `deploy/` files.

## D-071 — Anonymous statistics at stats.lapi.sh

- **Status:** accepted (owner request and design approval, 2026-10-22; design in the
  project) · **Affects:** the new service `services/stats` (`https://stats.lapi.sh`,
  `/home/www/lapi/stats`, port 8791), servers.lapi.sh (players online by hour), the
  launcher (`src-tauri/src/stats`, the feed's ad tile, launches from an ad, Settings →
  Privacy), the statistics of D-061 and the rule in CLAUDE.md. Replaces docs/spec/07's
  "No telemetry" at the owner's word (ignore the specification's telemetry and analytics
  parts).
- **Goal.** Know how many players there are and from where, which systems, versions and
  languages they have, whether they stay, and how each server ad does: impressions,
  reach, clicks, Play to the server and minutes there, with the impressions per erid the
  Russian ad register needs.
- **On by default.** The owner chose statistics on, with a switch in Settings → Privacy
  ("Anonymous statistics") that also covers the crash statistics of D-061. At the first
  start, and once after the update that brings it, a line at the bottom of the feed (or of
  Installations when the feed is off) says what is sent, with "Got it" and "Settings".
  Turning it off stops sending at once and erases what waits; a settings reset keeps the
  player's choice. "What is sent" shows the last ping and ad counts as they went, and
  today's ping before it goes. Crash reports still leave only when the player sends them.
- **Counting without IDs.** The launcher keeps only the day of its last ping and its
  install week (`old` for a launcher that ran before statistics). Once a UTC day it sends
  a ping with flags: the first of the day, of the week, of the month, ever. The service
  adds one to each of a few independent cuts under each flag (country, system, version,
  languages, country and time zone, memory and cores, kinds of accounts, install week),
  so no stored row describes one launcher, and retention comes from the install weeks.
  The ping has the launcher's version, channel and way installed, the OS, its version
  (Windows and macOS major version, a Linux distribution's ID) and the architecture, the
  interface and system languages, the time zone, memory and cores in ranges, and which
  kinds of accounts are signed in. Never an account, nick, address, path, install ID or a
  server's text. The country is Cloudflare's; the address is used only in memory for the
  limits, an hour.
- **Ad counts** are counted in the core for ads the feed handed out (D-069): shown (half
  of the tile on screen for a second while the window shows; again 5 minutes later at
  the earliest), menu, version list, address copied, report, Play, "needs a Microsoft
  account", installed or failed to install, the game started (with its version), joined
  the server, turned away by reason, ten minutes played, minutes on the server, and back
  to play a day or a week after the first time. Joining is read from the game's log by
  fixed words (a connection, then a chat line or two minutes without a refusal); the log
  never leaves the computer. A day of one ad is capped (300 shown, 50 clicks, 20 plays,
  1440 minutes). Counts go out in a batch every hour and at exit, kept 7 days offline.
- **Sending.** gzip JSON to `POST /v1/ping` and `POST /v1/counts` only, through the
  launcher's source rules (stats.lapi.sh is never mirrored), also on a metered
  connection (a few KiB). A random batch ID makes a resend count once. A proof of work
  (SHA-256 of `stats.lapi.sh|<batch>|<day>|<nonce>` with 20 leading zero bits, about a
  second; the service can ask for more, the launcher does up to 24) makes made-up
  requests cost. Refused requests are dropped, others wait for the next pass.
- **Service.** Rust and SQLite like the others, behind Cloudflare only, with nothing
  readable over HTTP. Limits before a body is read (per address, an IPv6 /64 counting as
  one, five times that per IPv6 /48, and overall), bodies of 8 and 64 KiB, values outside
  their words counted as `other`, caps on values and rows a day. Counts are taken only for
  ads the ads service showed that day (it is asked every 5 minutes on the server; it
  stays read only). A burst from one network (an IPv4 /24, an IPv6 /48) in an hour, more
  than 100 new launchers or 1,000 impressions of one ad, is counted apart and left out of
  the numbers and the ad register's. Sums by day are kept 2 years and then by month; ad
  counts 3 years; batch IDs 8 days; a backup daily.
- **Reading.** Only on the server, with `lapi-stats` (overview, players, cut, retention,
  ads, ad, ord; docs/STATS.md). No number goes into a workflow log, an issue or a pull
  request. servers.lapi.sh keeps each ad's players online by hour from when the ad is
  added and gives them to the statistics service on the server only.
- **Later stages** (not built yet): launches and crashes by version (the crash
  statistics of D-061 move here), play time, installs and the mirror, updates, error
  codes, tabs, the newcomer's way to a first game; a report file for an advertiser and
  `lapi-stats alerts`.
- **Server setup (owner):** a DNS record for stats.lapi.sh behind Cloudflare (proxied,
  with IP Geolocation on, so `CF-IPCountry` comes), then `deploy/bootstrap.sh` once, after
  the first deploy has uploaded the new `deploy/` files.

## D-072 — Interface concept A

- **Status:** accepted (owner's choice, 2026-10-22, from three concepts drawn for every
  screen) · **Affects:** the whole interface (`src/`), `docs/DESIGN.md`, the design skills
  in `.claude/skills/`, CLAUDE.md.
- **Decision:** The interface moves to concept A «Верстак»: a deep green left rail with
  the navigation and the player, a paper ground with white surfaces, one green accent,
  Golos Text and Noto Sans Arabic bundled with the app, stroke icons, skin heads as
  avatars and a launch bar at the bottom with the chosen installation and Play. No
  gradients, glows, blur or text in capitals. The owner also chose the feed as a mosaic,
  the mods catalog as tiles with collections and categories with icons, and the mod page
  with a sticky project card. `docs/DESIGN.md` is the brief.
- **Skills:** three design skills are committed so every session uses them:
  `frontend-design` (anthropics/skills), `impeccable` (pbakaus/impeccable, Apache-2.0) and
  `ui-ux-pro-max` (nextlevelbuilder/ui-ux-pro-max-skill, MIT); sources in
  `skills-lock.json`. After a review, what runs code we do not need is left out:
  impeccable's engine (it downloads a binary on first run) and its live mode (it injects a
  script and loosens the page CSP), and ui-ux-pro-max's tests, data validator and catalog
  snapshots. Its search script reads only its own CSV files. Each changed SKILL.md says
  so at the top. An update of a skill is reviewed the same way before it is committed.
- **Installation settings in place:** an installation's settings open in place of its
  details on the Installations screen (Installation settings in its toolbar, and the
  crash help's memory, Java and loader actions), with Back to details. Settings →
  Installations keeps the same forms (D-036, D-044 and the loader hint still hold), and
  the Java of an installation stays three choices (D-044). Fullscreen and launcher skins
  stay boxes, not switches, as they apply with Save.
- **Installed tab:** the modpack card, a toolbar whose end says what the last check or
  Update all found, and each folder's files as rows in a panel, their switches and
  buttons lined up. The screen keeps My requests and Add to the catalog at the top and the
  catalog's icon tabs (the board's text tabs and requests tab are not taken), the longer
  button names that say what they do (Open mods folder, In the catalog, Remove), and each
  file's size and world. A file's source and the set's dependency problems on the tab
  wait for data from the mods thread. A question (turn off a needed mod, remove a file)
  opens in place under its row; Cancel always closes it, and the focus goes back to what
  asked.
- **Order:** tokens and shell, then Installations and the launch bar, the new
  installation form, the feed, the catalog, the mod page and the remaining screens, each
  in its own pull request. Features the new screens need from a service go to the thread
  that owns it.
- **Verification:** screenshots in light, dark, Arabic and at the 820 px minimum width for
  each pull request; a Windows package for window changes (CLAUDE.md).

## D-073 — Versions Mojang's list does not have, from upd.lapi.sh

- **Status:** accepted (owner request, 2026-10-22) · **Affects:** the version picker (a
  new tab), installation of those versions, Worlds with Friends (versions with a
  server), `services/upd`, `data/extra-versions.json`, the Versions workflow.
- **Goal.** Players can play every April Fools version and every other build Mojang's
  version list leaves out: 2.0, the first builds Mojang replaced, Combat Tests, the
  1.18 and 1.19 experimental snapshots, the unobfuscated builds of late 2025, and the
  Classic, Indev, Infdev, Alpha, Beta and 1.0–1.5 builds kept by archives.
- **Owner's choices:** every such version, 2.0 included; the April Fools versions first,
  newest first, then the rest, newest first; no other domain in the launcher for these
  versions: our own list and every game jar on upd.lapi.sh; every jar checked for
  viruses first; versions with a server offered in Worlds with Friends behind a toggle
  of their own; our skins mod not offered on them for now.
- **The list.** `scripts/extra-versions.mjs` writes `data/extra-versions.json` from
  Mojang's list and the archive list at skyrising.github.io/mc-versions (which names
  the archives' files, from BetaCraft and Omniarchive, and builds left on Mojang's
  servers): 233 versions, 9 April Fools versions of Mojang's list among them. Versions
  whose client is the same file as one in Mojang's list, and servers without a client,
  are left out. A modern build keeps its own version file (or the file of the Mojang
  build it replaced); an archived old one takes the file of the nearest older Mojang
  version, so it starts the way Mojang starts that era. Log configurations are
  Mojang's current ones, fixed against Log4Shell. Every version file names its jars at
  `https://upd.lapi.sh/versions/files/<sha1>.jar` and is pinned by its SHA-1.
- **Publishing.** The Versions workflow downloads every jar from where it came from,
  checks SHA-1 and size, and scans all of them with ClamAV; a finding stops it. On
  main it signs the list with the update key (the key never goes to the server),
  uploads the jars upd.lapi.sh does not have, each the same bytes that were scanned,
  and `lapi-upd versions-files` and `versions` put files and list in place; the list
  goes in place only once every file it names is there, and never an older one.
  Launchers read `/versions/manifest.json`, check the signature with the built-in
  public key, then each version file and jar by SHA-1. Libraries, assets and Java are
  Mojang's, as for every version, with the mirror's fallback (D-062).
- **Worlds with Friends** needs 1.7.6 or newer (UUID whitelist), so the toggle offers the
  versions with a server from 1.7.6's day (2014-04-09) on, 45 of them from 15w14a to
  26w14a; 2.0 and older are left out there. Their ids have every shape, so the catalog
  says which they are: the launcher takes one only as an entry of the signed list, and a
  world made for one records it in `world.json` (`special`). Their servers read the
  properties in a snapshot's forms. The worlds service takes the same ids for rooms
  from `data/extra-world-versions.txt`, which `scripts/extra-versions.mjs` writes; a new
  list redeploys it.
- **Builds that crash at start** (owner, 2026-10-23): three Indev builds,
  in-20100124-2310, in-20100128-2304 and in-20100129-1452, end with a
  NullPointerException while building the starter house, with Mojang's own tweaker
  too. They stay in the list with a `note` (`crashes-at-start`, the only value the
  update tool and the launcher take); the picker marks them «Crashes at start», the
  form says the error is the game's own, and the Player test plays them only by id.
  The same note went on 1.21.11-pre1_unobfuscated (2026-10-23, as decided for these):
  on both test machines it ends with a NullPointerException while it applies the
  graphics preset on its first start, and the builds before and after it start.
