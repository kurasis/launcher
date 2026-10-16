# Developer guide

## Toolchain

| Tool                      | Version                                                            | Where it is pinned                                       |
| ------------------------- | ------------------------------------------------------------------ | -------------------------------------------------------- |
| Node.js                   | 24.21.0 (LTS "Krypton")                                            | `.nvmrc`, `package.json#engines` (`>=24.15.0 <25`)       |
| npm                       | 11.x (bundled with Node 24)                                        | `package-lock.json` (lockfile v3)                        |
| Rust                      | 1.97.0, with `rustfmt` and `clippy`                                | `rust-toolchain.toml` (rustup installs it automatically) |
| Tauri                     | `tauri` 2.12.4, `@tauri-apps/cli` 2.12.3, `@tauri-apps/api` 2.12.4 | `src-tauri/Cargo.lock`, `package-lock.json`              |
| React / Vite / TypeScript | 19.3.1 / 8.3.4 / 6.0.3                                             | `package-lock.json`                                      |

Always install JavaScript dependencies with `npm ci` and build Rust with `--locked` so the
committed lockfiles are used. End users need none of these tools.

## Native development dependencies

These are needed to build the app, not to run a packaged build.

**Linux (Ubuntu 24.04 / Debian 12):**

```sh
sudo apt-get install -y build-essential curl wget file libwebkit2gtk-4.1-dev libxdo-dev \
  libssl-dev libayatana-appindicator3-dev librsvg2-dev
```

**Windows 11 x64:** Microsoft C++ Build Tools ("Desktop development with C++") and the
Microsoft Edge WebView2 Runtime (preinstalled on Windows 11).

**macOS (Apple Silicon or Intel):** Xcode Command Line Tools (`xcode-select --install`).

The authoritative list is Tauri's prerequisites page: <https://v2.tauri.app/start/prerequisites/>.

## Scripts

All commands run from the repository root.

| Command                           | What it does                                                           |
| --------------------------------- | ---------------------------------------------------------------------- |
| `npm run tauri dev`               | Starts Vite on `localhost:1420` and the desktop app with hot reload    |
| `npm run dev`                     | Vite dev server only (UI without the Rust core; IPC calls fail)        |
| `npm run typecheck`               | `tsc -b` for app and tooling sources                                   |
| `npm run lint`                    | ESLint (type-checked rules, React hooks, no `dangerouslySetInnerHTML`) |
| `npm run format` / `format:check` | Prettier write / verify                                                |
| `npm test`                        | Vitest frontend tests (jsdom)                                          |
| `npm run build`                   | Typecheck and production frontend build into `dist/`                   |
| `npm run rust:fmt`                | `cargo fmt --check`                                                    |
| `npm run rust:lint`               | `cargo clippy --all-targets --locked -D warnings`                      |
| `npm run rust:test`               | `cargo test --locked` (unit, IPC and configuration tests)              |
| `npm run check`                   | All of the above checks in sequence (no release build)                 |
| `npm run tauri build`             | Release binary plus installers configured for the current OS           |
| `npx tauri build --no-bundle`     | Release binary only, at `src-tauri/target/release/kurasis-launcher`    |

CI (`.github/workflows/ci.yml`) runs the frontend checks once and the Rust checks plus a
release shell build on Linux x64, Windows x64, macOS arm64 and macOS x64.

## Configuration

| Variable                | When it is read                  | Purpose                                                                                    |
| ----------------------- | -------------------------------- | ------------------------------------------------------------------------------------------ |
| `KURASIS_MSA_CLIENT_ID` | Rust compile time                | Project-owned Microsoft public-client ID (non-secret). See [AUTH-SETUP.md](AUTH-SETUP.md). |
| `KURASIS_DATA_ROOT`     | App start, **debug builds only** | Uses another data directory, for tests and demonstrations. Release builds ignore it.       |

Example: `KURASIS_MSA_CLIENT_ID=<your-guid> npm run tauri dev`. Never put secrets, tokens or
passwords into environment files that are committed; `.env*` files are git-ignored.

## Project layout

The layout follows [docs/spec/02-ARCHITECTURE-AND-STORAGE.md](spec/02-ARCHITECTURE-AND-STORAGE.md).

| Path                                                                     | Responsibility                                                              |
| ------------------------------------------------------------------------ | --------------------------------------------------------------------------- |
| `src/app/`                                                               | Top-level layout and bootstrap                                              |
| `src/features/<area>/`                                                   | Feature UI: `accounts`, `instances`, `downloads`, `settings`, `diagnostics` |
| `src/components/`                                                        | Reusable accessible UI components                                           |
| `src/styles/`                                                            | Design tokens (`tokens.css`) and global styles                              |
| `src/lib/ipc.ts`                                                         | The only place that calls Tauri `invoke`                                    |
| `src/lib/contracts.ts`                                                   | TypeScript mirror of the Rust DTOs                                          |
| `src/lib/strings.ts`                                                     | User-facing English strings                                                 |
| `src-tauri/src/commands/`                                                | IPC handlers (validation and delegation only)                               |
| `src-tauri/src/{auth,catalog,install,runtime,launch,storage,lifecycle}/` | Core modules; each module doc comment names the phase that implements it    |
| `src-tauri/capabilities/`                                                | Window permissions; each command is granted explicitly                      |
| `src-tauri/tests/`                                                       | Integration tests (IPC through the mock runtime, security configuration)    |
| `tests/fixtures/`                                                        | Synthetic, non-secret test data (added from P04/P05)                        |
| `assets/app-icon.svg`                                                    | Source of the generated icons in `src-tauri/icons/`                         |

### Application data

The Rust core stores everything under the per-user application data directory
(`DATA_ROOT`, for example `~/.local/share/io.github.kurasis.launcher` on Linux), never in
the official `.minecraft` folder. The layout is defined in
[spec/02](spec/02-ARCHITECTURE-AND-STORAGE.md) and implemented in
`src-tauri/src/storage/paths.rs`. Only one launcher process may own a DATA_ROOT at a time.
To try the app without touching your real data:

```sh
npx tauri build --debug --no-bundle
KURASIS_DATA_ROOT="$PWD/.dev-data" ./src-tauri/target/debug/kurasis-launcher
```

### Lifecycle test harness (debug builds only)

Debug builds show a "Lifecycle test" panel on the About page. It starts a harmless copy of
the launcher as a child process (scenarios: 4 s, 12 s, immediate exit, crash, inherited
pipes, heavy output) to exercise window unloading and restoration exactly like a game run.
Release builds contain neither the panel's commands nor the child entry point.

### Adding an IPC command

1. Implement the handler in `src-tauri/src/commands/` and register it in `lib.rs`.
2. Add its name to the `AppManifest` list in `src-tauri/build.rs`; Tauri then generates an
   `allow-<command>` permission.
3. Grant that permission in `src-tauri/capabilities/default.json` and update
   `src-tauri/tests/security_config.rs`, which pins the exact permission list.
4. Add the DTO to `src/lib/contracts.ts` and a typed wrapper to `src/lib/ipc.ts`.

### Regenerating icons

```sh
rsvg-convert -w 1024 -h 1024 assets/app-icon.svg -o /tmp/icon-1024.png
npx tauri icon /tmp/icon-1024.png -o src-tauri/icons
# then delete the generated android/, ios/ and Windows Store (Square*/StoreLogo) files
```

## Running the GUI on a headless Linux machine

For automated screenshots the app runs under Xvfb, for example:

```sh
Xvfb :99 -screen 0 1280x800x24 &
DISPLAY=:99 dbus-run-session -- ./src-tauri/target/release/kurasis-launcher
```

AT-SPI and portal warnings in that environment are expected and harmless.
