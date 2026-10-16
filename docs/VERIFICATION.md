# Verification record

Actual results only. Each entry names the host, the commit or pull request and the kind of
evidence: **static** (lint/type/compile), **automated** (tests with fixtures or mock
runtime), **host GUI** (app run on the development host), **CI** (GitHub-hosted runners),
**live API** or **real game**. Nothing here is a target-machine qualification unless it
says so; the qualification matrix is completed in P13.

## Test environments

| Environment                                            | Available   | Used for                     | Notes                                                                                                |
| ------------------------------------------------------ | ----------- | ---------------------------- | ---------------------------------------------------------------------------------------------------- |
| Ubuntu 24.04.5 LTS x86_64 container (development host) | Yes         | Build, tests, GUI under Xvfb | No GPU, no window manager, no desktop session, no Secret Service vault. Not a qualification desktop. |
| GitHub-hosted `ubuntu-24.04`                           | Yes (CI)    | Rust checks + release build  | Compile/test only, no GUI run                                                                        |
| GitHub-hosted `windows-2025` (x64)                     | Yes (CI)    | Rust checks + release build  | Compile/test only, no GUI run                                                                        |
| GitHub-hosted `macos-15` (arm64)                       | Yes (CI)    | Rust checks + release build  | Compile/test only, no GUI run                                                                        |
| GitHub-hosted `macos-15-intel` (x64)                   | Yes (CI)    | Rust checks + release build  | Compile/test only, no GUI run                                                                        |
| Windows 11 x64 desktop                                 | **Missing** | Qualification (P12/P13)      | Owner to provide; record exact build                                                                 |
| macOS Apple Silicon desktop                            | **Missing** | Qualification                | Owner to provide; record macOS version                                                               |
| macOS Intel desktop                                    | **Missing** | Qualification                | Owner to provide; record macOS version                                                               |
| Ubuntu 24.04 desktop, X11 and Wayland sessions         | **Missing** | Qualification                | Owner to provide                                                                                     |
| Debian 12 desktop                                      | **Missing** | Qualification                | Owner to provide                                                                                     |

Exact target OS versions for qualification are recorded here before P13 starts.

## P01 — Project foundation

- **Host:** Ubuntu 24.04.5 LTS, x86_64, 4 vCPU, container without GPU. GUI checks ran under
  Xvfb (`1280x800x24`) with `dbus-run-session`.
- **Tools:** Node 24.21.0, npm 11.19.0, rustc/cargo 1.97.0, Tauri 2.12.4 (CLI 2.12.3, wry
  0.57.0, tao 0.37.1), WebKitGTK 4.1 2.52.6.
- **Branch / PR:** `claude/exciting-cori-pldz43`, [kurasis/launcher#1](https://github.com/kurasis/launcher/pull/1).

| Check                            | Command                                                                   | Evidence kind | Result                                                                                                                                                                                                                                                                                       |
| -------------------------------- | ------------------------------------------------------------------------- | ------------- | -------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------- |
| Dependency install from lockfile | `npm ci`                                                                  | static        | Pass, 0 vulnerabilities reported, no engine warnings                                                                                                                                                                                                                                         |
| TypeScript                       | `npm run typecheck`                                                       | static        | Pass                                                                                                                                                                                                                                                                                         |
| ESLint                           | `npm run lint`                                                            | static        | Pass, 0 warnings                                                                                                                                                                                                                                                                             |
| Prettier                         | `npm run format:check`                                                    | static        | Pass                                                                                                                                                                                                                                                                                         |
| Frontend tests                   | `npm test`                                                                | automated     | Pass, 8 tests in 2 files                                                                                                                                                                                                                                                                     |
| Frontend production build        | `npm run build`                                                           | static        | Pass                                                                                                                                                                                                                                                                                         |
| rustfmt                          | `npm run rust:fmt`                                                        | static        | Pass                                                                                                                                                                                                                                                                                         |
| Clippy                           | `npm run rust:lint`                                                       | static        | Pass, `-D warnings`                                                                                                                                                                                                                                                                          |
| Rust tests                       | `npm run rust:test`                                                       | automated     | Pass, 12 tests (6 unit, 1 IPC via Tauri mock runtime, 5 security configuration)                                                                                                                                                                                                              |
| Release build                    | `npx tauri build --bundles deb`                                           | static        | Pass on host (3 min 57 s); binary `src-tauri/target/release/kurasis-launcher`, `.deb` 1.77 MiB declaring `libwebkit2gtk-4.1-0, libgtk-3-0`                                                                                                                                                   |
| Release app opens                | Run release binary under Xvfb                                             | host GUI      | Pass: 1100×720 window "Kurasis Launcher"; React view renders under the production CSP and shows data from Rust `get_app_info` (Release, linux / x86_64, Tauri 2.12.4, Microsoft client ID "Not configured"). Screenshot: [p01-release-linux-x64.png](verification/p01-release-linux-x64.png) |
| Development loop                 | `npm run tauri dev`, then edit `src/lib/strings.ts` and `src/app/App.tsx` | host GUI      | Pass: app opened in debug mode ([before](verification/p01-dev-before.png)); changing the build-tag label and badge tone hot-reloaded without restart ([after](verification/p01-dev-hmr-after.png)); change reverted                                                                          |
| CI                               | `.github/workflows/ci.yml`                                                | CI            | See the CI section below                                                                                                                                                                                                                                                                     |

### CI results

Filled in from the pull request's checks before merge.

| Job                                  | Result  |
| ------------------------------------ | ------- |
| Frontend checks (ubuntu-24.04)       | Pending |
| Rust core linux-x64 (ubuntu-24.04)   | Pending |
| Rust core windows-x64 (windows-2025) | Pending |
| Rust core macos-arm64 (macos-15)     | Pending |
| Rust core macos-x64 (macos-15-intel) | Pending |

### Not verified in P01

- No GUI run on Windows or macOS; CI only compiles and tests there.
- No Wayland session, no real desktop environment, no screen reader run.
- No Microsoft, Xbox or Minecraft service call (no project client ID exists yet).
- Installers are not validated (P12). The `.deb` was built only to confirm the bundler runs.
