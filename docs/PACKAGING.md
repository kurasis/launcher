# Packaging and release metadata

How packages are built, tested and described (docs/spec/07, phase P12). User-facing
installation notes are in [INSTALL.md](INSTALL.md).

## Targets

| Target        | Package                   | Built on              | Notes                                                      |
| ------------- | ------------------------- | --------------------- | ---------------------------------------------------------- |
| windows-x64   | NSIS `*_x64-setup.exe`    | `windows-2025`        | Per-user install; WebView2 download bootstrapper (silent)  |
| windows-arm64 | NSIS `*_arm64-setup.exe`  | `windows-11-arm`      | Native arm64 build; the Windows 11 interface check (D-048) |
| macos-arm64   | `.app` in `*_aarch64.dmg` | `macos-15`            | Native build; minimum macOS 11.0                           |
| macos-x64     | `.app` in `*_x64.dmg`     | `macos-15-intel`      | Native build; minimum macOS 11.0                           |
| linux-x64     | `.deb`, `.AppImage`       | `debian:12` container | glibc 2.36 baseline; runtime dependencies generated        |

MSI and RPM are not produced (D-008). The bundle identifier `app.launcher`
names DATA_ROOT, the credential-store service and the single-instance namespace; changing
it needs a migration plan (D-001).

## Workflow

`.github/workflows/package.yml` runs on pull requests, pushes to `main` and manual
dispatch:

1. Each target job installs dependencies from lockfiles (`npm ci`, `cargo --locked`) and
   runs `tauri build --ci` for its bundles.
2. `scripts/package-metadata.mjs` copies the packages to `dist-packages/` and writes
   `SHA256SUMS` and `build-info.json` (product, identifier, app version, commit, target,
   time, signing status, whether a Microsoft client ID was configured, `rustc`, `cargo`,
   Node, Tauri CLI and glibc versions, CI run).
3. Smoke tests (`scripts/smoke/`):
   - Linux: the `.deb` is installed on fresh `debian:12` and `ubuntu:24.04` containers
     with only its declared dependencies (the script fails if Rust, Node or Java is
     present), started under Xvfb, removed and purged; user data must remain. The
     AppImage is started with `APPIMAGE_EXTRACT_AND_RUN=1`.
   - Windows: silent per-user install, uninstall entry, start, silent uninstall; user
     data must remain.
   - macOS: mount, copy the app, report architecture and signature, start, delete the
     app; user data must remain.
     A start passes when the process is still running after 20 seconds and holds the
     DATA_ROOT supervisor lock. CI runners are not clean consumer machines; see
     VERIFICATION.md for what each run proves.
4. Packages are uploaded as workflow artifacts (7 days for pull requests, 30 days
   otherwise). With the update key configured, a `release` job also builds the signed
   update archive `launcher-<version>-release.tar.gz` ([RELEASING.md](RELEASING.md)).
   Nothing creates a GitHub release; publishing an update is a manual step.

Local equivalent for one target: `npx tauri build --bundles deb,appimage` (Linux; set
`APPIMAGE_EXTRACT_AND_RUN=1` in containers), then
`node scripts/package-metadata.mjs --target linux-x64 --out dist-packages --signing unsigned <files>`.

## Build configuration

| Variable                                                              | Kind                                  | Effect                                                                   |
| --------------------------------------------------------------------- | ------------------------------------- | ------------------------------------------------------------------------ |
| `LAUNCHER_MSA_CLIENT_ID`                                              | Repository variable (public)          | Microsoft client ID compiled in; blank = sign-in unavailable             |
| `LAUNCHER_UPDATE_PUBKEY`                                              | From the `UPDATE_PUBLIC_KEY` variable | The update signing key compiled in; blank = no update checks (D-063)     |
| `LAUNCHER_UPDATE_URL`, `LAUNCHER_ACCOUNT_URL`, `LAUNCHER_FRIENDS_URL` | Local test builds only                | Another update, account or friends server; release builds never set them |

## Signing (owner actions)

Packages are unsigned until the owner provides credentials. Signing status is reported
separately from functional test results in `build-info.json` and VERIFICATION.md.

**macOS (required for public delivery).** Add repository secrets; the workflow reads them
only on pushes to `main` and manual runs, never on pull requests:

| Secret                                        | Content                                                   |
| --------------------------------------------- | --------------------------------------------------------- |
| `APPLE_CERTIFICATE`                           | Base64 of the exported "Developer ID Application" `.p12`  |
| `APPLE_CERTIFICATE_PASSWORD`                  | Password of that `.p12`                                   |
| `APPLE_SIGNING_IDENTITY`                      | e.g. `Developer ID Application: Name (TEAMID)`            |
| `APPLE_ID`, `APPLE_PASSWORD`, `APPLE_TEAM_ID` | Apple ID, app-specific password and team for notarization |

Tauri signs the app and DMG when the certificate and identity are present and notarizes
when the Apple ID values are present too. Requires a paid Apple Developer Program
membership.

**Windows (strongly recommended).** Not wired yet: the owner chooses a code-signing
option (an OV/EV certificate, or Azure Trusted Signing) and adds it as
`bundle.windows.signCommand` or `certificateThumbprint` with the matching secrets, read
only on trusted events. SmartScreen reputation is not solved by signing alone.

**Linux.** Packages are not signed. If an apt repository is published later, sign its
metadata there.

## Upgrades and uninstall

- Upgrades install over the previous version; DATA_ROOT is outside the install location
  on every OS and is never touched by the installer.
- The NSIS uninstaller removes DATA_ROOT only if the user ticks "Delete the application
  data" (off by default, ignored for `/S` silent uninstall and updates). `apt remove` and
  `apt purge` do not touch home directories. Deleting the macOS app or the AppImage leaves
  data in place.
- Stored credentials live in the OS credential store and are removed by **Sign out**,
  not by uninstalling.

## Third-party notices

Before any public release, collect licences of bundled Rust crates and npm packages (for
example with `cargo about` and `license-checker`) and ship them with the packages. The
private Java runtime is downloaded at install time from Adoptium and carries its own
notices; no Minecraft files are bundled.
