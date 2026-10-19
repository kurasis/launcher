# Release notes

> **A snapshot.** This page describes the 0.1.0 qualification build as it was then and
> is not updated: much has changed since (Java on the computer may be used, D-044;
> in-app updates, D-063; the launcher account, D-064). What changed and why is in
> [DECISIONS.md](DECISIONS.md); each released version's notes for players are in
> [`release-notes/<version>.json`](../release-notes/README.md).

Publishing a release is a separate decision by the project owner. Nothing here has been
published; packages exist only as CI workflow artifacts.

## 0.1.0 — private qualification build (not released)

### Contents

- Installations of the supported Minecraft: Java Edition releases, each with its own game
  folder (worlds, options, screenshots) and settings for memory and window size.
- Installation and repair from the official sources only (Mojang metadata and files,
  Adoptium Temurin Java), with SHA-1/SHA-256 verification, a shared content-addressed
  cache, resumable downloads, cancel, and repair that never touches worlds or options.
- A private Java runtime per required major version; the system Java is never used.
- Launch compiler with argument files, encoding checks and redacted logs; game
  supervision that closes the launcher window during play and brings it back on exit.
- Microsoft sign-in and access checks (implemented and fixture-tested; not yet usable,
  see below).
- Settings, diagnostics with a sanitized export, and an About page with the official
  Minecraft EULA and help links.
- Packages: Windows NSIS (per user), macOS DMG (Apple silicon and Intel), Linux `.deb`
  and AppImage, each with `SHA256SUMS` and `build-info.json`.

### Compatibility policy

The launcher installs only versions on its built-in list, pinned to the version
descriptor hash it was qualified with. Instances never upgrade by themselves.

| Minecraft | Descriptor SHA-1                           | Java           | Status                                                                    |
| --------- | ------------------------------------------ | -------------- | ------------------------------------------------------------------------- |
| 1.21.1    | `499680d659b7a0ccb2fa9a809c345c5baf2ace7d` | Temurin 21 JRE | Pending verification: installs and arguments verified; play needs sign-in |
| 26.3      | `11f1aefe473eefba0af62e3286c33b692807b2eb` | Temurin 25 JRE | Pending verification: installs and arguments verified; play needs sign-in |

### Platforms

| Target                   | Package          | Built on            | Tested                                                                         |
| ------------------------ | ---------------- | ------------------- | ------------------------------------------------------------------------------ |
| Windows 10/11 x64        | NSIS setup       | windows-2025 (CI)   | CI runner: install, start, uninstall keeps data                                |
| macOS 11+ Apple silicon  | DMG              | macos-15 (CI)       | CI runner: mount, start, delete keeps data                                     |
| macOS 11+ Intel          | DMG              | macos-15-intel (CI) | CI runner: mount, start, delete keeps data                                     |
| Linux x64 (glibc ≥ 2.36) | `.deb`, AppImage | debian:12 (CI)      | Fresh Debian 12 and Ubuntu 24.04: install, start, purge, upgrade 0.1.0 → 0.1.1 |

All packages are unsigned. No consumer machine has been tested yet.

### Package references

Packages are the artifacts of the `Package` workflow
(`.github/workflows/package.yml`) on the commit being released; each artifact set holds
its own `SHA256SUMS` and `build-info.json`. Verified runs are listed in
[VERIFICATION.md](VERIFICATION.md) (P12 onwards). Locally built Linux packages and their
hashes are recorded in the P12 section; they came from a working tree with uncommitted
changes and are not release candidates.

### Known limitations

- Sign-in and play are unavailable until the project owner registers a Microsoft
  application ID and obtains Minecraft service access ([AUTH-SETUP.md](AUTH-SETUP.md)).
- Unsigned packages: SmartScreen and Gatekeeper warnings ([INSTALL.md](INSTALL.md)).
- Linux: two WebKitGTK file descriptors per window unload/return cycle stay open until
  the launcher exits (D-029).
- No automatic updater. No license chosen for the source yet.
