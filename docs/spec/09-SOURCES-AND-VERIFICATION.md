# 09 — Source register and verification evidence

Research date: **2026-10-16**. Links and metadata describe the observation at preparation time; integrations must be revalidated during implementation. This file separates live public-data checks from source-code observations and authenticated operations that were not performed.

## Verification summary

| Item | What was checked | Result / limit |
| --- | --- | --- |
| Mojang version catalog | Actual HTTPS GET and JSON parse | Succeeded; observed latest release `26.3`, latest snapshot `26.4-snapshot-4`; snapshots excluded from MVP |
| Two version descriptors | Actual GET and SHA-1 against catalog | Both matched; Java/runtime/argument/library metadata inspected |
| Asset index and one asset | Actual GET, index hash and object hash | Both matched; entire game asset set was not downloaded |
| Adoptium runtime metadata | Eight API queries: Java 21/25 × four platform targets | All returned a matching JRE package; runtime archives were not downloaded or executed |
| Prism authentication source | Exact files at commit `60bcc2694d7433e2d2a657fbf62b0d022f9116e0` | Current endpoint and parsing behavior inspected; no user credentials used |
| Official Microsoft OAuth docs | Public protocol documentation | PKCE and native-browser flow reviewed; project registration not created |
| Minecraft API review article | Page reachable | Article body unavailable in retrieval; no approval process or deadline confirmed |
| Desktop binaries / actual game | Not built or launched | All real app acceptance tests remain Pending |

## Official game artifact repositories

These are upstream download services, not Git repositories. They are sufficient to discover the files required for a vanilla client installation.

| Purpose | URL |
| --- | --- |
| Version manifest | <https://piston-meta.mojang.com/mc/game/version_manifest_v2.json> |
| Version-specific metadata | Follow the manifest `url` and validate its `sha1` |
| Client and logging data | Metadata-declared URLs under <https://piston-data.mojang.com/> |
| Libraries | Metadata-declared URLs under <https://libraries.minecraft.net/> |
| Asset objects | <https://resources.download.minecraft.net/> plus hash prefix/full hash |

Do not use a generic GitHub Minecraft client repository or a third-party mirror as a source of executable game files. Do not download server JARs or mappings for this client MVP.

## Exact version observations

These values are useful to compare a resolver implementation with the researched inputs. They are not a requirement to pin historical URLs forever; a live catalog can change a descriptor hash. A specific installation pins its chosen verified descriptor.

### Minecraft 1.21.1

- Descriptor: <https://piston-meta.mojang.com/v1/packages/499680d659b7a0ccb2fa9a809c345c5baf2ace7d/1.21.1.json>
- Descriptor SHA-1: `499680d659b7a0ccb2fa9a809c345c5baf2ace7d`; matched the catalog: `true`.
- Java metadata: component `java-runtime-delta`, major `21`.
- Declared main class: `net.minecraft.client.main.Main`.
- Total library records before platform filtering: `97`.
- Client: <https://piston-data.mojang.com/v1/objects/30c73b1c5da787909b2f73340419fdf13b9def88/client.jar>
- Client bytes: `26836906`; SHA-1: `30c73b1c5da787909b2f73340419fdf13b9def88`. Client bytes were not fetched during research.
- Asset index ID: `17`; URL: <https://piston-meta.mojang.com/v1/packages/15200809663f45954c2c9a33ce88849e61aaaecc/17.json>
- Asset index SHA-1: `15200809663f45954c2c9a33ce88849e61aaaecc`; declared bytes: `449557`.

### Minecraft 26.3

- Descriptor: <https://piston-meta.mojang.com/v1/packages/11f1aefe473eefba0af62e3286c33b692807b2eb/26.3.json>
- Descriptor SHA-1: `11f1aefe473eefba0af62e3286c33b692807b2eb`; matched the catalog: `true`.
- Java metadata: component `java-runtime-epsilon`, major `25`.
- Declared main class: `net.minecraft.client.main.Main`.
- Total library records before platform filtering: `114`.
- Client: <https://piston-data.mojang.com/v1/objects/e877b6a07acd633fb3bb475002175cec036e7b87/client.jar>
- Client bytes: `41483720`; SHA-1: `e877b6a07acd633fb3bb475002175cec036e7b87`. Client bytes were not fetched during research.
- Asset index ID: `34`; URL: <https://piston-meta.mojang.com/v1/packages/e787e8afd149a4cefa89d341b91f5aed8f5d93a0/34.json>
- Asset index SHA-1: `e787e8afd149a4cefa89d341b91f5aed8f5d93a0`; declared bytes: `597035`.

### Asset integrity probe

The 1.21.1 index at <https://piston-meta.mojang.com/v1/packages/15200809663f45954c2c9a33ce88849e61aaaecc/17.json> was fetched and its SHA-1 matched. A single small object named `icons/icon_128x128.png` was fetched:

- Object URL: <https://resources.download.minecraft.net/b6/b62ca8ec10d07e6bf5ac8dae0c8c1d2e6a1e3356>
- Received size: `9101` bytes.
- SHA-1: `b62ca8ec10d07e6bf5ac8dae0c8c1d2e6a1e3356`; matched expected hash: `true`.

The asset itself is not included in this specification archive. This probe confirms the content-addressed URL convention for one actual object, not every object or every platform.

## Java runtime metadata checks

Use the API result `binary.package`, not `binary.installer`. Package links can redirect to GitHub release assets. Checksums are provider-declared SHA-256 values; the archives were not downloaded to independently hash or execute them.

| Java | OS | Architecture | Observed release | Metadata |
| --- | --- | --- | --- | --- |
| 21 | windows | x64 | `jdk-21.0.12.1+1` | [Verified query](https://api.adoptium.net/v3/assets/latest/21/hotspot?architecture=x64&image_type=jre&os=windows&vendor=eclipse) |
| 21 | linux | x64 | `jdk-21.0.12.1+1` | [Verified query](https://api.adoptium.net/v3/assets/latest/21/hotspot?architecture=x64&image_type=jre&os=linux&vendor=eclipse) |
| 21 | mac | x64 | `jdk-21.0.12.1+1` | [Verified query](https://api.adoptium.net/v3/assets/latest/21/hotspot?architecture=x64&image_type=jre&os=mac&vendor=eclipse) |
| 21 | mac | aarch64 | `jdk-21.0.12.1+1` | [Verified query](https://api.adoptium.net/v3/assets/latest/21/hotspot?architecture=aarch64&image_type=jre&os=mac&vendor=eclipse) |
| 25 | windows | x64 | `jdk-25.0.4.1+1` | [Verified query](https://api.adoptium.net/v3/assets/latest/25/hotspot?architecture=x64&image_type=jre&os=windows&vendor=eclipse) |
| 25 | linux | x64 | `jdk-25.0.4.1+1` | [Verified query](https://api.adoptium.net/v3/assets/latest/25/hotspot?architecture=x64&image_type=jre&os=linux&vendor=eclipse) |
| 25 | mac | x64 | `jdk-25.0.4.1+1` | [Verified query](https://api.adoptium.net/v3/assets/latest/25/hotspot?architecture=x64&image_type=jre&os=mac&vendor=eclipse) |
| 25 | mac | aarch64 | `jdk-25.0.4.1+1` | [Verified query](https://api.adoptium.net/v3/assets/latest/25/hotspot?architecture=aarch64&image_type=jre&os=mac&vendor=eclipse) |

### Example portable runtime package

- Package name: `OpenJDK21U-jre_x64_windows_hotspot_21.0.12.1_1.zip`.
- Provider URL: <https://github.com/adoptium/temurin21-binaries/releases/download/jdk-21.0.12.1%2B1/OpenJDK21U-jre_x64_windows_hotspot_21.0.12.1_1.zip>
- Provider-declared size: `48999141` bytes.
- Provider-declared SHA-256: `d35f31e712f0fcf6ac5a093edc90204fbff22f720ba3950bd09d331d5e621636`.

Select the matching OS/architecture/major dynamically and persist the returned build receipt; do not hard-code this example package into every target.

## Source-code repositories for implementation reference

These repositories are for protocol/architecture research or direct dependencies, not sources of licensed Minecraft client binaries.

| Repository | Appropriate use | Reuse caution |
| --- | --- | --- |
| [tauri-apps/tauri](https://github.com/tauri-apps/tauri) | Desktop lifecycle and Rust/JS bridge | Use official stable 2.x packages and current docs |
| [tauri-apps/plugins-workspace](https://github.com/tauri-apps/plugins-workspace) | Single-instance, dialogs, opener and other official plugins | Enable only necessary permissions |
| [facebook/react](https://github.com/facebook/react) | UI dependency | Lock a compatible stable version |
| [vitejs/vite](https://github.com/vitejs/vite) | Frontend build tooling | Development/build only, no shipped dev server |
| [microsoft/TypeScript](https://github.com/microsoft/TypeScript) | Type checking | Compile-time tooling |
| [adoptium/api.adoptium.net](https://github.com/adoptium/api.adoptium.net) | Runtime discovery API | API metadata was tested; archive execution was not |
| [adoptium/temurin21-binaries](https://github.com/adoptium/temurin21-binaries) | Java 21 upstream distribution | Follow API-selected packages and preserve notices |
| [adoptium/temurin25-binaries](https://github.com/adoptium/temurin25-binaries) | Java 25 upstream distribution | Follow API-selected packages and preserve notices |
| [PrismLauncher/PrismLauncher](https://github.com/PrismLauncher/PrismLauncher) | Current Minecraft auth and launcher behavior | GPL/mixed historical file licenses; do not copy blindly |
| [modrinth/code](https://github.com/modrinth/code) | Additional desktop-launcher architectural reference | Monorepo has mixed licensing; not audited here for direct reuse |
| [CmlLib/CmlLib.Core.Auth.Microsoft](https://github.com/CmlLib/CmlLib.Core.Auth.Microsoft) | Registration and auth caveats | .NET library, not a Rust dependency; older endpoint examples require revalidation |

### Pinned authentication references

- [AuthFlow.cpp](https://github.com/PrismLauncher/PrismLauncher/blob/60bcc2694d7433e2d2a657fbf62b0d022f9116e0/launcher/minecraft/auth/AuthFlow.cpp) — Sequence and Minecraft XSTS relying party.
- [steps/MSAStep.cpp](https://github.com/PrismLauncher/PrismLauncher/blob/60bcc2694d7433e2d2a657fbf62b0d022f9116e0/launcher/minecraft/auth/steps/MSAStep.cpp) — Microsoft consumer authorization/token endpoints and observed scope profile.
- [steps/XboxUserStep.cpp](https://github.com/PrismLauncher/PrismLauncher/blob/60bcc2694d7433e2d2a657fbf62b0d022f9116e0/launcher/minecraft/auth/steps/XboxUserStep.cpp) — Xbox RPS user-token exchange.
- [steps/XboxAuthorizationStep.cpp](https://github.com/PrismLauncher/PrismLauncher/blob/60bcc2694d7433e2d2a657fbf62b0d022f9116e0/launcher/minecraft/auth/steps/XboxAuthorizationStep.cpp) — XSTS exchange, identity consistency and restriction errors.
- [steps/LauncherLoginStep.cpp](https://github.com/PrismLauncher/PrismLauncher/blob/60bcc2694d7433e2d2a657fbf62b0d022f9116e0/launcher/minecraft/auth/steps/LauncherLoginStep.cpp) — Current `/launcher/login` exchange.
- [steps/EntitlementsStep.cpp](https://github.com/PrismLauncher/PrismLauncher/blob/60bcc2694d7433e2d2a657fbf62b0d022f9116e0/launcher/minecraft/auth/steps/EntitlementsStep.cpp) — Current `/entitlements/license` request.
- [steps/MinecraftProfileStep.cpp](https://github.com/PrismLauncher/PrismLauncher/blob/60bcc2694d7433e2d2a657fbf62b0d022f9116e0/launcher/minecraft/auth/steps/MinecraftProfileStep.cpp) — Authenticated Java profile and missing-profile distinction.
- [Parsers.cpp](https://github.com/PrismLauncher/PrismLauncher/blob/60bcc2694d7433e2d2a657fbf62b0d022f9116e0/launcher/minecraft/auth/Parsers.cpp) — Play-access versus permanent-ownership parsing.

These exact files were retrieved directly from the repository at the pinned commit. Source behavior is not proof that a newly registered third-party client ID is authorized. The implementation must complete G0 using its own application. No copied third-party source code is included in the archive.

## Framework, identity and packaging documentation

| Topic | Source |
| --- | --- |
| Tauri architecture and frontend integration | <https://v2.tauri.app/start/> |
| Frontend tooling | <https://v2.tauri.app/start/frontend/vite/> |
| Rust lifecycle | <https://docs.rs/tauri/latest/tauri/enum.RunEvent.html> |
| Webview window lifecycle | <https://v2.tauri.app/reference/javascript/api/namespacewebviewwindow/> |
| Single instance | <https://v2.tauri.app/plugin/single-instance/> |
| Capabilities | <https://v2.tauri.app/security/capabilities/> |
| WebView differences | <https://v2.tauri.app/reference/webview-versions/> |
| Windows installer / WebView2 | <https://v2.tauri.app/distribute/windows-installer/> |
| Debian packaging | <https://v2.tauri.app/distribute/debian/> |
| AppImage and baseline compatibility | <https://v2.tauri.app/distribute/appimage/> |
| Future signed updater | <https://v2.tauri.app/plugin/updater/> |
| OAuth authorization code + PKCE | <https://learn.microsoft.com/en-us/entra/identity-platform/v2-oauth2-auth-code-flow> |
| Microsoft redirect constraints | <https://learn.microsoft.com/en-us/entra/identity-platform/reply-url> |
| Adoptium OpenAPI UI | <https://api.adoptium.net/q/swagger-ui/> |
| Rust credential-vault adapter candidate | <https://docs.rs/keyring/latest/keyring/> |
| Client-ID access caveat | <https://cmllib.github.io/CmlLib.Core-wiki/en/auth.microsoft/xboxauthnet.game.msal/clientid/> |
| Historical purchase-check/Game Pass caveat | <https://cmllib.github.io/CmlLib.Core-wiki/en/auth.microsoft/cmllib.core.auth.microsoft/jeauthenticator/> |
| Minecraft API review entry point | <https://help.minecraft.net/hc/en-us/articles/16254801392141> |
| Minecraft EULA | <https://www.minecraft.net/en-us/eula> |
| Minecraft usage guidelines | <https://www.minecraft.net/en-us/usage-guidelines> |

## Implementation-time revalidation checklist

1. Re-fetch the game catalog and descriptors; compare required fields and native argument behavior, not just IDs.
2. Re-resolve the required Java major for each target and verify archive checksums after real download.
3. Confirm own client registration, exact scopes and current Minecraft service contract with an authorized account.
4. Test current-access semantics with a licensed purchase, no-access account and active subscription; distinguish profile setup from ownership.
5. Pin compatible dependencies and rerun packaging checks against their actual requirements.
6. Record real tests honestly. Public metadata access, source inspection, compilation and mocked tests are different evidence levels.

## Research boundaries

No credentials were requested or used, no registration was submitted, no application was built, no game was launched, no binary/JAR/runtime archive was distributed, and no approval/signing claim is made. Download URLs and protocol details can change. The specification supplies concrete implementation inputs and explicit validation gates rather than a guarantee that unauthenticated research alone produces a shippable launcher.

## Version 1.2.0 audit references and evidence limits

Audit date: 2026-10-17 (Asia/Dubai). The original public metadata observations above are preserved as dated observations; this revision does not claim a fresh complete game download or authenticated API validation.

| Reference | Audit use |
| --- | --- |
| [Java 25 launcher manual](https://docs.oracle.com/en/java/javase/25/docs/specs/man/java.html) | Argfile encoding/escaping, working-directory paths and JDK_JAVA_OPTIONS behavior; actual Java 21/25 target tests still required |
| [Tauri ExitRequestApi](https://docs.rs/tauri/latest/tauri/struct.ExitRequestApi.html) | Avoid assuming prevent_exit can cancel every programmatic exit/restart path |
| [Tauri AppHandle](https://docs.rs/tauri/latest/tauri/struct.AppHandle.html) | Main-thread scheduling and exit/restart behavior; pin the actual implementation dependency |
| [Microsoft redirect constraints](https://learn.microsoft.com/en-us/entra/identity-platform/reply-url) | Registered host/path, localhost port exception and IP-literal registration caveats |

The retained pinned Prism source was reviewed for request-field and entitlement semantics. Its raw auth-debug logging is explicitly not part of this design. These are protocol implementation references, not proof that this project's registration is approved.

All 27 original Markdown files were audited and revised. See [AUDIT-REPORT.md](AUDIT-REPORT.md) for issue-level coverage and [CHANGELOG.md](CHANGELOG.md) for the release summary. No application tests were marked passed by this documentation audit.

## Second review (1.2.1)

The second review uses the retained verified descriptors and their two hash-verified asset indexes to compare shared objects. No client JAR, library JAR or Java runtime was executed. New metadata comparisons and their limits are recorded in [document 10](10-MULTIVERSION-AND-CACHE.md). The Java launcher manual and Tauri single-instance documentation were reopened to check classpath and plugin boundaries; the new cache keys, OS-backed DATA_ROOT lock and regression requirements are this project's design decisions, not claimed upstream guarantees.
