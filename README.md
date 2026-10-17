# Kurasis Launcher

An independent desktop launcher for Minecraft: Java Edition, built with Tauri 2, React,
TypeScript and Vite on a Rust core. "Kurasis Launcher" is a provisional name.

> Kurasis Launcher is an independent project. It is not affiliated with or endorsed by
> Mojang Studios or Microsoft. It does not include or redistribute any Minecraft game files.

## Status

Implementation follows the phased specification in [`docs/spec/`](docs/spec/README.md).
The single progress tracker is [`docs/spec/PROGRESS.md`](docs/spec/PROGRESS.md).

Current state: **P01 — project foundation**. The app is a runnable desktop shell with a
React view that talks to the Rust core over typed IPC. Sign-in, installation and game
launch are not implemented yet, and the UI does not pretend otherwise.

Microsoft sign-in needs a project-owned Microsoft application (client) ID and Minecraft
service access approval. Both are owner actions; see [`docs/AUTH-SETUP.md`](docs/AUTH-SETUP.md).

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

| Document                                     | Purpose                                                               |
| -------------------------------------------- | --------------------------------------------------------------------- |
| [docs/DEVELOPMENT.md](docs/DEVELOPMENT.md)   | Developer setup, scripts and project layout                           |
| [docs/AUTH-SETUP.md](docs/AUTH-SETUP.md)     | Microsoft application registration and service access (owner actions) |
| [docs/INSTALL.md](docs/INSTALL.md)           | Installing, updating and uninstalling the packages                    |
| [docs/PACKAGING.md](docs/PACKAGING.md)       | Package builds, checksums, smoke tests and signing (owner actions)    |
| [docs/DECISIONS.md](docs/DECISIONS.md)       | Decision records and deviations                                       |
| [docs/VERIFICATION.md](docs/VERIFICATION.md) | Actual check results per phase, host and commit                       |
| [docs/spec/](docs/spec/README.md)            | Product specification v1.2.1 and phase plan                           |

Minecraft is a trademark of Mojang Synergies AB. Use of the game is subject to the
[Minecraft EULA](https://www.minecraft.net/en-us/eula).
