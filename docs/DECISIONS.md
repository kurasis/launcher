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
