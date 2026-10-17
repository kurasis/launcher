# 01 — Product requirements and scope

## Goal

Provide a small, customizable launcher for people with legitimate Minecraft: Java Edition access. Installation and runtime management should feel like one operation. The player should not need a terminal, Rust, Node.js, npm, a Java SDK, or knowledge of JVM flags.

The launcher replaces the official launcher's interface. Minecraft remains an unmodified external Java process with its own window. In-game menus, rendering, account services and server authentication remain Minecraft responsibilities.

## MVP requirements

| ID | Requirement |
| --- | --- |
| FR01 | Sign in through the system browser with Microsoft and show the resulting Java profile. |
| FR02 | Verify current Java play access using authenticated Minecraft service data before each launch. |
| FR03 | Explain no license, missing profile, expired login, unavailable service and unsupported app registration as different states. |
| FR04 | List the curated supported vanilla releases using Mojang metadata; never silently upgrade an existing instance. |
| FR05 | Create an installation with a display name and immutable selected game version. |
| FR06 | Automatically fetch matching Java, client JAR, libraries, native artifacts, assets and logging configuration. |
| FR07 | Display real phase, bytes and progress; support cancellation, retry and file-level recovery. |
| FR08 | Validate downloads and repair missing/corrupt managed files without modifying worlds or settings. |
| FR09 | Configure maximum Java heap, window dimensions and UI behavior while playing. |
| FR10 | Launch exactly one game at a time; show actionable failures and bounded logs. |
| FR11 | Unload the GUI during gameplay and restore it after normal exit or crash. |
| FR12 | Store game installations separately from the official `.minecraft` directory. |
| FR13 | Package for Windows x64, macOS x64/arm64 and Linux x64. |
| FR14 | Persist settings, selected installation and basic UI state across restarts. |
| FR15 | Sign out locally, removing saved credentials and disabling new launches. |
| FR16 | Open the selected installation's game folder and export redacted diagnostics on user request. |

## Deliberate scope limits

No Forge/Fabric/NeoForge/Quilt installation, mods catalog, modpacks, snapshots, legacy pre-modern argument formats, version inheritance, Bedrock, server hosting, skin editing, embedded game window, friend/social feed, telemetry, cloud sync, multiple concurrent games, or offline launch in MVP. No automatic launcher updater. No arbitrary JVM-argument text box or custom Java executable selector initially.

Keep interfaces extensible, but do not implement speculative plugin systems. Manual installation deletion and data-directory relocation are deferred to avoid adding destructive flows. (Relocation was added later as a copy that keeps the old folder, with a portable mode: DECISIONS.md D-043.) Users may open their installation folder; explain that deleting it manually deletes its worlds.

## Supported release policy

The initial qualification set is `1.21.1` (Java 21) and `26.3` (Java 25), confirmed in public metadata during research. Fetch the manifest dynamically but intersect results with a shipped compatibility list. Do not compare dotted versions numerically: modern identifiers include a different year-based scheme. The live `latest.release` is informative, not automatically supported.

Keep a version as “pending verification” until the complete real-launch matrix passes. During development, one version may be the first vertical slice; the final initial release must either qualify both targets or explicitly document an owner-approved narrowed scope. Historical client metadata can be updated: pin the descriptor hash per installation. Repair restores these exact inputs; it never upgrades metadata or Java builds. Metadata/runtime upgrades are deferred maintenance features. A new instance may use newly qualified inputs; an existing instance stays pinned.

## Platform policy

| Target | Initial qualification baseline | Notes |
| --- | --- | --- |
| Windows x64 | Supported Windows 11 installation | Windows 10 support may be added only with separate actual testing. |
| macOS arm64 | Supported macOS release on Apple Silicon | Native launcher, runtime and compatible natives; Rosetta not a default requirement. |
| macOS x64 | Supported macOS release on Intel | Separate build and real test; do not infer from arm64 success. |
| Linux x64 | Ubuntu 24.04 and Debian 12 glibc desktops; qualify each explicitly | X11 and a Wayland session must be tested; distribution support is a documented list. |

These are product targets, not claims about the minimum supported OS of Tauri or every Minecraft release. Actual game graphics, driver and Java requirements can be stricter. Publish the tested OS versions and hardware in the release notes. Alpine/musl, 32-bit and Linux/Windows ARM are outside scope.

## User journeys

### First use

Open app → choose “Sign in with Microsoft” → browser authentication → verified access and profile → create installation → install → Play → game window opens → launcher UI unloads → game exits → launcher returns to the same installation.

Downloading and browsing supported versions may remain available without a verified account, but the real-game launch command must always enforce the access gate. First-run UI should encourage sign-in before a large download.

### Returning user

Open app → restore selected installation → refresh account when needed → verify current access → check required files → play. Unchanged verified files are reused. A valid local installation is not proof of current entitlement.

### Failure recovery

Network outage → keep installed files → show retryable connection error. Invalid token → ask for sign-in. Corrupt artifact → redownload only affected managed files. Game crash → restore GUI with exit status and redacted log tail. Service outage must never be presented as “you do not own Minecraft.”

## Defaults

- System theme with dark/light override; no video backgrounds or permanent animations.
- 4 GiB maximum heap on machines with at least 8 GiB RAM; conservative lower default on smaller systems with a warning when available RAM is inadequate. Validate against measured physical memory; never promise every supported release works at the lower value.
- Initial heap 512 MiB unless compatibility tests require another value; ensure initial heap does not exceed maximum.
- Windowed game, 1280 × 720, adjustable within validated bounds.
- Eight concurrent downloads, bounded to 1–16 in internal configuration.
- During game: unload GUI and reopen after exit. Alternative: keep launcher open.
- English UI for MVP, externalized strings for later localization.

## Compatibility qualification without a development deadlock

The compatibility policy records version ID, accepted descriptor hashes, target OS/architecture and qualification status. P05–P12 may use a clearly marked **qualification build** with the two fixed candidate releases enabled for owner-operated tests. It uses real authentication, integrity checks and normal source policy; it cannot authorize an arbitrary version or fake account. Public release builds expose only qualified entries. A changed upstream descriptor hash is a new candidate, not silently trusted as already tested. Existing instances retain their locked hash and can restore it from the recorded URL.

P13 qualifies the candidate packages, updates the support policy, then rebuilds release packages with the same locked code/dependencies and runs a focused install/login/play/return smoke test of the final artifacts. Record both artifact hashes and the exact policy-only change. Do not claim that an untested final package was the tested candidate.

MVP serializes shared-data activity: one installation/repair or one launch/game at a time across all instances. Downloads within one install remain concurrent. This keeps shared runtimes and assets stable while Minecraft runs.

## Multiple installations are a required MVP journey

Users can keep both supported versions installed, create two isolated instances of the same version, and alternate between them. There is no arbitrary all-version support claim and no concurrent game execution. Common artifact content is reused; worlds/options are never shared automatically. The exact cache keys, minimum additional downloads and regression cases are normative in [document 10](10-MULTIVERSION-AND-CACHE.md).
