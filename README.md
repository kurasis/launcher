# Sferum Launcher

An independent desktop launcher for Minecraft: Java Edition, built with Tauri 2, React,
TypeScript and Vite on a Rust core. "Sferum Launcher" is a provisional name.

> Sferum Launcher is an independent project. It is not affiliated with or endorsed by
> Mojang Studios or Microsoft. It does not include or redistribute any Minecraft game files.

![Sferum Launcher: installations and the Play screen](docs/images/home.png)

## Status

Implementation follows the phased specification in [`docs/spec/`](docs/spec/README.md).
The single progress tracker is [`docs/spec/PROGRESS.md`](docs/spec/PROGRESS.md); the
evidence is in [`docs/VERIFICATION.md`](docs/VERIFICATION.md).

**What works (verified on Linux x64):** creating installations of the supported releases
(Minecraft 1.21.1 with Java 21 and 26.3 with Java 25), installing them from the official
Mojang and Adoptium sources with digest verification, a shared verified cache, cancel,
resume and repair, a private Java runtime per version, the launch compiler (real Java
accepts the real arguments of both versions), game supervision with window unload and
return, diagnostics export, settings, and packages for Windows, macOS and Linux built and
smoke-tested in CI.

**What does not work yet:** signing in and playing. Microsoft sign-in needs a
project-owned Microsoft application (client) ID and Minecraft service access approval,
both owner actions ([`docs/AUTH-SETUP.md`](docs/AUTH-SETUP.md)). Without them the Play
button explains that sign-in is required; no fake session is ever used in release
builds. For automated tests, debug builds can launch the real game in Minecraft's demo
mode with a test profile ([`docs/DEVELOPMENT.md`](docs/DEVELOPMENT.md#test-profile-debug-builds-only), D-030).

**Not qualified yet:** real Windows, macOS and Wayland machines, real accounts and signed
packages. The MVP is not complete until those checks pass (P13 in
[PROGRESS.md](docs/spec/PROGRESS.md)).

### Known limitations

- Packages are unsigned private qualification builds; Windows SmartScreen and macOS
  Gatekeeper warn before opening them ([`docs/INSTALL.md`](docs/INSTALL.md)).
- On Linux each unload/return cycle of the window keeps two WebKitGTK file descriptors
  open until the launcher exits (about 490 game sessions per launcher run; D-029).
- The AppImage uses the host's glibc (2.36 or newer), graphics and audio libraries.
- The launcher has no automatic updater; install newer packages over older ones.
- No license has been chosen for the source yet (D-010).

## Quick start

Prerequisites (details in [`docs/DEVELOPMENT.md`](docs/DEVELOPMENT.md)): Node.js 24.21.0
(`.nvmrc`), rustup (the toolchain is pinned in `rust-toolchain.toml`) and the Tauri system
dependencies for your OS.

```sh
npm ci
npm run tauri dev      # run the desktop app with hot reload
npm run check          # typecheck, lint, format check, frontend + Rust tests, clippy
npm run tauri build    # release build and installers for the current OS
```

## Documentation

| Document                                           | Purpose                                                               |
| -------------------------------------------------- | --------------------------------------------------------------------- |
| [docs/DEVELOPMENT.md](docs/DEVELOPMENT.md)         | Developer setup, scripts and project layout                           |
| [docs/AUTH-SETUP.md](docs/AUTH-SETUP.md)           | Microsoft application registration and service access (owner actions) |
| [docs/INSTALL.md](docs/INSTALL.md)                 | Installing, updating and uninstalling the packages                    |
| [docs/PACKAGING.md](docs/PACKAGING.md)             | Package builds, checksums, smoke tests and signing (owner actions)    |
| [docs/RELEASE-NOTES.md](docs/RELEASE-NOTES.md)     | Changes, compatibility and package references per version             |
| [docs/TROUBLESHOOTING.md](docs/TROUBLESHOOTING.md) | Common problems and what to do                                        |
| [docs/DECISIONS.md](docs/DECISIONS.md)             | Decision records and deviations                                       |
| [docs/VERIFICATION.md](docs/VERIFICATION.md)       | Actual check results per phase, host and commit                       |
| [docs/spec/](docs/spec/README.md)                  | Product specification v1.2.1 and phase plan                           |

Minecraft is a trademark of Mojang Synergies AB. Use of the game is subject to the
[Minecraft EULA](https://www.minecraft.net/en-us/eula).
