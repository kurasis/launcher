# Implementation progress

This tracker is intentionally initialized as **not_started**. Creating the specification does not implement any application feature. The coding agent updates this file in place at `SPEC_ROOT/PROGRESS.md` (recommended repository `docs/spec/PROGRESS.md`). Keep it beside PHASES.md so relative links work; do not maintain a second tracker.

Allowed states: `not_started`, `in_progress`, `blocked`, `done`. A partially implemented phase stays `in_progress` or `blocked`; use Notes to distinguish finished code from missing live verification.

| Phase | Status | Evidence / commit | Blocker / notes |
| --- | --- | --- | --- |
| [P01](phases/P01-PROJECT-FOUNDATION.md) | done | [PR #1](https://github.com/kurasis/launcher/pull/1); host checks + CI on 4 targets, see [VERIFICATION.md](../VERIFICATION.md#p01--project-foundation) | Host GUI checked on Linux x64 only; Microsoft client ID not supplied (does not block P01) |
| [P02](phases/P02-CORE-STORAGE-AND-IPC.md) | done | [PR #2](https://github.com/kurasis/launcher/pull/2); 89 Rust + 19 frontend tests, CI 5/5, host GUI demo; see [VERIFICATION.md](../VERIFICATION.md#p02--rust-state-storage-and-typed-ipc) | GUI demo on Linux x64 only; other targets via CI tests |
| [P03](phases/P03-LIFECYCLE-SPIKE.md) | done | [PR #3](https://github.com/kurasis/launcher/pull/3); lifecycle scenarios + 40 cycles on Linux x64 host, CI 5/5; see [VERIFICATION.md](../VERIFICATION.md#p03--early-window-and-process-lifecycle-proof) | Window behaviour unverified on Windows/macOS/Wayland (P13); memory growth ≈85 KiB/cycle tracked for A23 |
| [P04](phases/P04-AUTHENTICATION-AND-ACCESS.md) | blocked | [PR #4](https://github.com/kurasis/launcher/pull/4) merged; 44 synthetic auth tests, CI 5/5; see [VERIFICATION.md](../VERIFICATION.md#p04--microsoft-login-and-current-game-access) and [AUTH-CONTRACT.md](../AUTH-CONTRACT.md) | Live positive sign-in impossible: no project-owned Microsoft client ID / Minecraft API approval (owner action). P05–P08 proceed meanwhile |
| [P05](phases/P05-CATALOG-AND-INSTALL-PLAN.md) | done | [PR #5](https://github.com/kurasis/launcher/pull/5); live catalog + plans for both baselines on 4 targets, independent cross-check, CI 5/5; see [VERIFICATION.md](../VERIFICATION.md#p05--minecraft-metadata-and-deterministic-install-planning) | Metadata success is not game compatibility (P13) |
| [P06](phases/P06-DOWNLOAD-ENGINE.md) | in_progress | 18 fixture tests + live smoke (cancel, retry, reuse); see [VERIFICATION.md](../VERIFICATION.md#p06--verified-cancellable-download-engine) | Waiting for CI; UI progress wiring lands with install in P08 (D-022) |
| [P07](phases/P07-MANAGED-JAVA.md) | in_progress | 25 automated tests; live install + probe of Temurin 21/25 on Linux x64, live extraction of Windows/macOS packages; see [VERIFICATION.md](../VERIFICATION.md#p07--private-java-runtime-installation) | Waiting for CI; Java execution on Windows/macOS unverified (P13) |
| [P08](phases/P08-COMPLETE-GAME-INSTALLER.md) | not_started | — | — |
| [P09](phases/P09-REAL-GAME-LAUNCH.md) | not_started | — | — |
| [P10](phases/P10-BACKGROUND-AND-RETURN.md) | not_started | — | — |
| [P11](phases/P11-COMPLETE-PRODUCT-UI.md) | not_started | — | — |
| [P12](phases/P12-CROSS-PLATFORM-PACKAGING.md) | not_started | — | — |
| [P13](phases/P13-FINAL-QUALIFICATION.md) | not_started | — | — |
| [P14](phases/P14-HANDOVER.md) | not_started | — | — |

## External dependencies

| Dependency | State | Required owner contribution |
| --- | --- | --- |
| Project-owned Microsoft public client and service access | Not supplied (tracked in [AUTH-SETUP.md](../AUTH-SETUP.md); does not block P01–P03, P05–P08) | Supply registered client ID and confirm accepted auth configuration/access |
| Real owned / no-access / active subscription test accounts | Unverified | Owner-operated tests; never put credentials into this file |
| Target OS/hardware test environments | Only an Ubuntu 24.04 container host and GitHub-hosted CI runners are available; no target desktops (see [VERIFICATION.md](../VERIFICATION.md#test-environments)) | Identify available Windows, macOS Intel/arm64 and Linux sessions |
| Release identity and signing | Provisional identity `io.github.kurasis.launcher` / "Kurasis Launcher" ([D-001](../DECISIONS.md#d-001--provisional-application-identity)); no signing | Choose stable app identity and supply signing configuration when distributing |

## Last completed phase report

```text
Phase: P05 — Minecraft metadata and deterministic install planning
Status: done
Implemented: public HTTPS client with exact per-source host allowlists, checked redirects
  and public-address resolution; catalog fetch with ETag revalidation, 6 h freshness and
  stale fallback; curated list gated by descriptor SHA-1; verified byte-for-byte metadata
  cache; last-match rules engine; pure planner (GamePlan + RuntimeRequirement, canonical
  content paths, ordered classpath, deduplicated assets); list_supported_versions and
  preview_install_plan with a metadata-only estimate in the new-installation form.
Checks: 37 Rust tests + 3 frontend tests; independent Python cross-check of library
  selection for 8 version/target cases; live catalog and plans for both baselines on 4
  targets; GUI demo on the Linux x64 host; CI 5/5.
Demo: Installations -> New installation -> pick a version -> "Estimate download".
Evidence limits: metadata only; game compatibility is P09/P13.
Blockers: none for P05. P04 stays blocked on the owner-supplied client ID.
Next eligible phase: P06 (download engine), then P07, P08
```

## Decisions / deviations

Record deliberate changes with reason, impacted FR/G/A identifiers and verification evidence. Never silently shrink supported platforms or waive missing tests.

## Required final account evidence

| Acceptance | Live result | Phase responsible |
| --- | --- | --- |
| A03: own-client positive authentication | Unverified | P04; reconfirm at P13 |
| A04: actual no-access account denied | Unverified | P13; may be collected in P04 |
| A05: active subscription accepted | Unverified | P13; may be collected in P04 |
| A06: profile/setup and service failures | Unverified | P13; fixtures start in P04 |

P04's local completion criteria and P13's final evidence requirements are distinct. See document 08. Specification version 1.2.1 changes documentation only; every implementation phase remains not_started.
