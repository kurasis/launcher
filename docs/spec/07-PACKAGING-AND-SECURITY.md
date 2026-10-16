# 07 — Packaging, security and delivery

## End-user dependencies

| OS | Launcher GUI dependencies | Distribution plan |
| --- | --- | --- |
| Windows | Microsoft WebView2 Runtime | NSIS `setup.exe`, per-user installation; bootstrap WebView2 if missing |
| macOS | System WKWebView | Signed/notarized `.app` in `.dmg`; native x64 and arm64 builds |
| Linux | WebKitGTK 4.1, GTK and platform libraries | `.deb` with dependency metadata and an AppImage; RPM after separate packaging validation |

Users do not install Rust, Cargo, Node, npm, Vite or TypeScript. Java is downloaded privately by the launcher. The application's minimum OS is determined jointly by Tauri, its dependencies, bundled Java and the selected Minecraft release; publish actual qualification results rather than framework minimums alone.

For Windows, keep Tauri's supported WebView2 installation mode enabled. The normal bootstrapper needs internet if the runtime is missing. An offline installer is an optional separate larger package. Do not ship a bare portable executable as “no dependencies required.”

For Linux, generate runtime package dependencies rather than shipping developer `-dev` packages. Build on the oldest qualified baseline that has Tauri 2's WebKitGTK 4.1 requirements. An AppImage is not universally independent of glibc, graphics drivers, FUSE availability or desktop services. Test fresh supported systems; document launch/extraction fallback if required. Ensure the credential-vault fallback works without requiring a particular tray extension.

References: [Windows installer](https://v2.tauri.app/distribute/windows-installer/), [Debian packages](https://v2.tauri.app/distribute/debian/), [AppImage limitations](https://v2.tauri.app/distribute/appimage/), [WebView providers](https://v2.tauri.app/reference/webview-versions/).

## Build and CI

Pin a compatible Rust toolchain and Node LTS version; choose the concrete versions during implementation from current framework prerequisites. Commit `Cargo.lock` and the chosen JS lockfile. Document native developer dependencies separately from end-user dependencies.

Required project scripts: frontend dev, Tauri dev, typecheck, lint, frontend test, Rust test and release build. Suggested names: `npm run dev`, `npm run tauri dev`, `npm run typecheck`, `npm run lint`, `npm test`, `cargo test --manifest-path src-tauri/Cargo.toml`, `npm run tauri build`. The agent must implement and verify the actual scripts rather than merely listing commands that do not exist.

CI jobs cover Windows x64, macOS x64, macOS arm64 and Linux x64. Use native runners or verified cross-compilation tooling; a successful cross-build does not replace a target-machine smoke test. CI performs dependency install with lockfile enforcement, formatting, lint/type checks, core tests and build. Package jobs publish artifacts only from trusted release events; never expose signing secrets to untrusted pull requests.

Archive release package checksums, app version, commit, dependency/toolchain versions and compatibility list. Supply app icons generated for this project. Do not commit `node_modules`, build output, downloaded game files, runtime archives or real user account data.

Unsigned developer builds and privately tested MVP artifacts may exist before signing is configured. Public-ready macOS delivery requires a working signing/notarization workflow; Windows signing is strongly recommended for distribution. Keep signing gates distinct from gameplay functionality and report both honestly. Do not describe OS reputation prompts as solved by Tauri itself.

## Trust boundaries

- Production webview loads packaged local UI only. No remote page receives Tauri capabilities.
- Start with minimal capabilities and grant only required window, dialog and opener actions. Application commands must validate inputs independently; capability configuration alone does not make custom commands safe.
- Keep networking and privileged filesystem/process operations in Rust. Do not expose generic shell, arbitrary HTTP or broad filesystem APIs to React.
- Use a restrictive production CSP compatible with Tauri IPC and local assets; no remote scripts or `unsafe-eval`. Keep Vite development allowances separate.
- Open external HTTPS destinations through a Rust allowlist for sign-in, official account/support and the project's release page. Reject arbitrary schemes and userinfo URLs.
- Separate credentialed auth HTTP clients from unauthenticated artifact clients. Disable credential forwarding across redirects; do not send Bearer tokens to Mojang asset or Adoptium/GitHub download hosts.
- Validate transport schemes, exact host boundaries, redirect count and paths. Do not use checks such as `host.contains("minecraft.net")`. Reject loopback/private-network artifact destinations in production. Public endpoints come from the trusted source adapters, not frontend input.
- Handle legitimate Adoptium GitHub/CDN redirects with documented exact hosts or narrowly scoped domain boundaries. Revalidate redirect destinations, never disable TLS verification to make a download work.
- Use generated UUIDs as instance/run identifiers. Display names are not directory paths. Handle spaces, Unicode and Windows path-length constraints in storage and subprocess calls.
- No arbitrary shell execution, HTML injection from logs, Markdown raw HTML from upstream text, or `dangerouslySetInnerHTML` for error bodies.

## Network host categories

The production source policy includes `login.microsoftonline.com`, Xbox token hosts and `api.minecraftservices.com` for auth; `piston-meta.mojang.com`, `piston-data.mojang.com`, `libraries.minecraft.net`, `resources.download.minecraft.net` for game data; and `api.adoptium.net` plus validated package destinations such as `github.com` and GitHub release-asset CDN hosts for runtime downloads. Browser authentication may navigate through Microsoft's own login hosts and is handled by the external browser, not the download allowlist.

Host changes are explicit source-adapter maintenance. A failing allowlist does not justify enabling arbitrary downloads or copying tokens into another service.

## Diagnostics and privacy

No telemetry or automatic crash upload in MVP. Export is user-initiated, with an explanation of included files. Exclude account secrets, full command lines, argfiles, worlds, screenshots and unrelated paths. Redact tokens and obvious user-directory identifiers from launcher logs. Never log HTTP authorization headers, token request forms or raw auth response bodies, even at debug level in a production build.

Retain account display metadata only as needed. Uninstalling the application should preserve game data by default; any future data-removal choice must clearly identify worlds and be explicit. Do not register application startup at login by default.

## License and branding treatment

Fetch Minecraft content directly from its declared upstreams at installation time; do not bundle or mirror game files inside the launcher repository/releases. Identify the launcher as an independent project and link the official Minecraft terms. Retain runtime/dependency notices and review redistribution obligations for any bundled dependency before release.

Reference implementations are not drop-in code licenses. Prism includes GPL-3.0 and differently licensed historical files; Modrinth's monorepo has mixed licensing. Consult exact file/package license and provenance before copying code. Prefer implementing the documented protocol behavior independently or reusing compatible licensed libraries. This specification does not grant permission to reuse third-party branding or copyrighted game assets.

Official references: [Minecraft EULA](https://www.minecraft.net/en-us/eula), [usage guidelines](https://www.minecraft.net/en-us/usage-guidelines), [Tauri capabilities](https://v2.tauri.app/security/capabilities/).

## Updates

MVP launcher updates are manual through an HTTPS release page or newly downloaded installer. Installation must preserve DATA_ROOT. Do not implement an unsigned executable self-updater. If automatic updates are added later, use Tauri's signed update mechanism, securely held signing keys and compatible per-target artifacts; game metadata downloads and launcher binary updates are separate mechanisms.

## Qualification and release identity

P12 produces private qualification packages; P13 establishes supported combinations and verifies final release artifacts as described in document 01. Record exact Windows/macOS builds and Linux distribution/display-session versions. Ubuntu 22.04 may be evaluated as a build environment, but it is not automatically an advertised runtime target. Validate native dependencies against both qualified Linux distributions and avoid claiming universal AppImage portability.

Choose the stable application identifier before the first distributed package: changing it changes DATA_ROOT/vault/single-instance namespaces. A later identity change requires an explicit migration plan, never a silent new empty profile. Package upgrade and uninstall tests cover worlds, settings and account references. Provide the in-app Sign out action to remove stored credentials before uninstall; do not promise that uninstall itself revokes remote sessions.

Production artifact requests validate each resolved/connected IP as public in addition to hostname/redirect checks. Reject private/loopback/link-local targets after DNS resolution, including redirect hops; ensure the chosen transport cannot re-resolve to an unchecked address. Apply bounded metadata and TLS policies consistently. The OAuth loopback listener is a distinct, intentional exception and is not an artifact-download destination.

## Shared data boundary

The supported DATA_ROOT is a local per-user filesystem with the required locking and replacement semantics; network/synced shared roots are not an MVP guarantee. The supervisor acquires the OS-backed DATA_ROOT lock before any mutation, independently of the single-instance UI plugin. Two installations of the launcher with the same stable identity must not create two writers. Do not add a force-unlock UI that deletes lock files while another supervisor might be alive.
