# Implementation progress

This tracker is intentionally initialized as **not_started**. Creating the specification does not implement any application feature. The coding agent updates this file in place at `SPEC_ROOT/PROGRESS.md` (recommended repository `docs/spec/PROGRESS.md`). Keep it beside PHASES.md so relative links work; do not maintain a second tracker.

Allowed states: `not_started`, `in_progress`, `blocked`, `done`. A partially implemented phase stays `in_progress` or `blocked`; use Notes to distinguish finished code from missing live verification.

| Phase | Status | Evidence / commit | Blocker / notes |
| --- | --- | --- | --- |
| [P01](phases/P01-PROJECT-FOUNDATION.md) | done | [PR #1](https://github.com/kurasis/launcher/pull/1); host checks + CI on 4 targets, see [VERIFICATION.md](../VERIFICATION.md#p01--project-foundation) | Host GUI checked on Linux x64 only; Microsoft client ID not supplied (does not block P01) |
| [P02](phases/P02-CORE-STORAGE-AND-IPC.md) | done | [PR #2](https://github.com/kurasis/launcher/pull/2); 89 Rust + 19 frontend tests, CI 5/5, host GUI demo; see [VERIFICATION.md](../VERIFICATION.md#p02--rust-state-storage-and-typed-ipc) | GUI demo on Linux x64 only; other targets via CI tests |
| [P03](phases/P03-LIFECYCLE-SPIKE.md) | not_started | — | — |
| [P04](phases/P04-AUTHENTICATION-AND-ACCESS.md) | not_started | — | — |
| [P05](phases/P05-CATALOG-AND-INSTALL-PLAN.md) | not_started | — | — |
| [P06](phases/P06-DOWNLOAD-ENGINE.md) | not_started | — | — |
| [P07](phases/P07-MANAGED-JAVA.md) | not_started | — | — |
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
Phase: P02 — Rust state, storage and typed IPC
Status: done
Implemented: validated DATA_ROOT with OS-backed lifetime lock; atomic versioned JSON
  documents with corrupt-file preservation, settings backup and read-only newer schemas;
  canonical digest-keyed cache paths; settings/accounts/instance/install-lock models with
  lock-authoritative reconciliation; AppState with epoch/revision events, MutationRequest
  idempotency and the global activity permit; error envelope, secret types and redaction;
  validated memory/resolution/name bounds; shipped compatibility list; typed frontend store
  and installations/settings UI.
Checks: 80 Rust unit + 9 integration tests (incl. cross-process lock), 19 frontend tests,
  clippy/rustfmt/typecheck/lint/format - pass; CI 5/5 jobs pass.
Demo: create an installation, change memory, restart: values and selection restored; a
  second process against the same DATA_ROOT shows DATA_ROOT_LOCKED.
Evidence limits: GUI demo on the Linux x64 host under Xvfb; Windows/macOS via CI tests only.
Blockers: none for P02. External: Microsoft client ID / service approval; target desktops.
Next eligible phase: P03 (P04 and P05 are also eligible)
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
