# Minecraft Java Launcher — MVP implementation brief

Specification version: **1.2.1** · Original research: 2026-10-16 · Audit: 2026-10-17 (Asia/Dubai) · Language: English

Build a real desktop Minecraft: Java Edition launcher using **Tauri 2, React, TypeScript, Vite, and a Rust core**. Users sign in with their own Microsoft account, prove current access to Java Edition, install a supported vanilla release and the correct private Java runtime, play in the game's separate window, and return to the launcher when the game exits. The launcher UI must be easy to redesign with web technologies.

This archive contains a specification, not an implemented launcher. Public metadata and selected source code were checked; no authenticated Microsoft/Minecraft flow, game launch, or packaged application was tested during preparation. See the precise evidence in [09 — Sources and verification](09-SOURCES-AND-VERIFICATION.md).

## How to give this to a development chatbot

1. Extract this folder intact as `docs/spec/` in the implementation repository. This directory is `SPEC_ROOT`; run build commands from the repository root. Keep the single authoritative tracker at `SPEC_ROOT/PROGRESS.md`.
2. Give the chatbot [START-HERE.md](START-HERE.md) and [PHASES.md](PHASES.md). Start with P01; each phase file contains a ready-to-use English implementation prompt. Ask for one phase at a time or authorize the full sequence.
3. Provide your own registered public-client Microsoft application ID and approved Minecraft service access when available. A client ID is configuration, not a client secret. Do not send account passwords or refresh tokens to the chatbot.
4. Require working code and evidence for the acceptance checklist, rather than a UI-only prototype.

## Reading order

| File | Purpose |
| --- | --- |
| [START-HERE.md](START-HERE.md) | Instructions and boundaries for the coding agent |
| [PHASES.md](PHASES.md) | P01–P14 execution map, dependencies and continuation prompts |
| [PROGRESS.md](PROGRESS.md) | Single implementation tracker, initially not started |
| [CHANGELOG.md](CHANGELOG.md) | Changes in specifications 1.2.1 and 1.2.0 |
| [AUDIT-REPORT.md](AUDIT-REPORT.md) | Historical first audit (1.2.0) |
| [SECOND-AUDIT.md](SECOND-AUDIT.md) | Second review, concrete corrections and current validation limits |
| `phases/P01-…md` through `phases/P14-…md` | Individual phase tasks, deliverables, checks and chatbot prompts |
| [01-PRODUCT-AND-SCOPE.md](01-PRODUCT-AND-SCOPE.md) | Product behavior, MVP boundaries, support policy |
| [02-ARCHITECTURE-AND-STORAGE.md](02-ARCHITECTURE-AND-STORAGE.md) | Modules, repository structure, disk layout and data ownership |
| [03-AUTHENTICATION.md](03-AUTHENTICATION.md) | Microsoft/Xbox/Minecraft flow and license gate |
| [04-INSTALLATION-AND-SOURCES.md](04-INSTALLATION-AND-SOURCES.md) | Download repositories, resolver, integrity, Java, repair |
| [05-LAUNCH-AND-LIFECYCLE.md](05-LAUNCH-AND-LIFECYCLE.md) | JVM arguments, game process, unloading and restoring GUI |
| [06-UI-AND-IPC.md](06-UI-AND-IPC.md) | Screens, states, typed commands, events and errors |
| [07-PACKAGING-AND-SECURITY.md](07-PACKAGING-AND-SECURITY.md) | Installers, OS dependencies, CI, trust boundaries |
| [08-IMPLEMENTATION-AND-ACCEPTANCE.md](08-IMPLEMENTATION-AND-ACCEPTANCE.md) | Build sequence, meaningful tests, definition of done |
| [09-SOURCES-AND-VERIFICATION.md](09-SOURCES-AND-VERIFICATION.md) | Source register, exact probes, research limitations |
| [10-MULTIVERSION-AND-CACHE.md](10-MULTIVERSION-AND-CACHE.md) | Exact sharing/locking contract and cross-version regression scenarios |

## Decisions already made

- Local desktop app; no product backend service required for MVP.
- Web frontend packaged locally, with Rust owning networking, credentials, disk and processes.
- Vanilla only; curated supported releases, initially **1.21.1** and **26.3**, subject to real launch verification. These are baseline targets, not a promise that every future release works.
- Windows x64, macOS Intel and Apple Silicon, Linux x64. No mobile, Windows ARM64, Linux ARM64, 32-bit or emulation requirement in MVP.
- One signed-in account at a time, multiple isolated local installations, one running game process.
- Online access verification required for every new launch; no offline/cracked/demo bypass.
- Managed private Eclipse Temurin runtime from Adoptium; Java major selected from the Minecraft version metadata.
- Destroy the main webview during gameplay by default, retain the Rust supervisor, recreate the UI after game exit.
- Manual launcher updates for MVP; automatic updates are an extension, not a hidden release dependency.

## External release gates

The code can be developed without publishing credentials, but the MVP cannot be declared complete without a successful real login using this project's client ID, a positive and negative license test, and a real game smoke test on each claimed platform. Public distribution also needs the owner's chosen application identity and signing setup. Do not disguise these dependencies as finished work.
