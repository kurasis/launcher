# Implementation phases — P01 to P14

This is the authoritative execution order for the coding agent. The numbered specification documents remain the detailed requirements. Every phase has its own task file and a copy-ready English prompt.

## Identifier conventions

- **P01–P14:** implementation phases in this document.
- **FR01–FR16:** functional requirements in document 01 (renamed from the earlier P identifiers to avoid ambiguity).
- **G0–G5:** quality gates in document 08. They are checkpoints, not a second competing task sequence.
- **A01–A26:** final acceptance checks in document 08.

## Execution map

| Phase | Concrete result | Required prior phases |
| --- | --- | --- |
| [P01 — Project foundation and external prerequisites](phases/P01-PROJECT-FOUNDATION.md) | A runnable desktop shell with a real React view. | None |
| [P02 — Rust state, storage and typed IPC](phases/P02-CORE-STORAGE-AND-IPC.md) | Authoritative Rust application state and typed frontend contracts. | P01 |
| [P03 — Early window and process lifecycle proof](phases/P03-LIFECYCLE-SPIKE.md) | A small lifecycle test harness and reusable window controller. | P02 |
| [P04 — Microsoft login and current game access](phases/P04-AUTHENTICATION-AND-ACCESS.md) | Real auth adapter, credential storage and minimal account flow. | P02 |
| [P05 — Minecraft metadata and deterministic install planning](phases/P05-CATALOG-AND-INSTALL-PLAN.md) | Manifest source, rules engine, pure GamePlan and RuntimeRequirement. | P02 |
| [P06 — Verified, cancellable download engine](phases/P06-DOWNLOAD-ENGINE.md) | Production DownloadTransport and executor with typed progress/events. | P02, P05 |
| [P07 — Private Java runtime installation](phases/P07-MANAGED-JAVA.md) | RuntimeProvider, safe runtime extractor and immutable runtime receipts. | P05, P06 |
| [P08 — Complete installation, recovery and repair](phases/P08-COMPLETE-GAME-INSTALLER.md) | End-to-end vanilla installer and repair flow. | P05, P06, P07 |
| [P09 — Real authenticated Minecraft launch](phases/P09-REAL-GAME-LAUNCH.md) | Production launch compiler and ProcessSupervisor. | P04, P08 |
| [P10 — Unload GUI during play and return after exit](phases/P10-BACKGROUND-AND-RETURN.md) | Production background supervisor and automatic return behavior. | P03, P09 |
| [P11 — Complete web UI, settings and diagnostics](phases/P11-COMPLETE-PRODUCT-UI.md) | Complete real UI with reusable styling/component structure. | P04, P08, P10 |
| [P12 — Windows, macOS and Linux packages](phases/P12-CROSS-PLATFORM-PACKAGING.md) | Target installers/packages and reproducible build jobs. | P11 |
| [P13 — Full compatibility, security and performance qualification](phases/P13-FINAL-QUALIFICATION.md) | Filled docs/VERIFICATION.md and A01–A26 checklist with evidence. | P12 |
| [P14 — Final handover and reproducibility](phases/P14-HANDOVER.md) | Source repository, usable package references and complete handover docs. | P13 |

## How to execute

1. Start at P01. Read the selected phase and linked requirements before implementing it. Keep the initial scope vanilla-only and preserve the existing specification.
2. Aim for one reviewable phase at a time. A request to implement P05 means implement P05 and report its result; it does not authorize silently skipping its prerequisites.
3. If asked to implement the whole MVP, continue through eligible phases without asking for confirmation between routine reversible steps.
4. After each phase, update the single [PROGRESS.md](PROGRESS.md) at `SPEC_ROOT/PROGRESS.md`, and provide the short report below.
5. Mark a phase done only when its deliverables and required checks actually pass. Distinguish code completion from externally blocked live tests.
6. Do not wait on branding choices that can use explicit temporary defaults. Do not invent service approval, signing keys, account credentials or test results.

## Handling a blocked P04

Investigate auth registration from P01 and implement P04 early. If own-client approval or a real test account is unavailable, P05, P06, P07 and P08 can proceed after their listed prerequisites; they do not require a licensed launch. The UI-only work in P11 and installer configuration in P12 can also be prepared as partial work when their specific inputs exist, but those phases cannot be marked done prematurely.

P04 requires its implementation, deterministic account-decision tests and at least one real positive own-client authentication. P09 requires completed P04/P08 plus fresh access verification; there is no dependency exception. Missing real no-access/owned/subscription matrix evidence is tracked against A03–A06 and blocks P13/G5, not P04 after its stated checks pass. No fixture or borrowed client ID may satisfy a live test. P13/P14 cannot be called complete while mandatory evidence is missing.

P03 lifecycle proof is intentionally early; P10 integrates it with the real game. Final real-machine qualification is P13, so earlier host-only success must remain labelled host-only.

## Per-phase report template

```text
Phase: Pxx — title
Status: done | in_progress | blocked
Implemented: concrete components and behavior
Checks: commands/scenarios + actual pass/fail/blocked results
Demo: steps to observe the result
Evidence limits: host-only / synthetic / live API / real game / packaged target
Blockers: non-secret reason + required owner action, if any
Next eligible phase: Pxx
```

## Starting prompt

```text
Read README.md, START-HERE.md and PHASES.md. Implement P01 now according to phases/P01-PROJECT-FOUNDATION.md. Produce working code, run its checks, update PROGRESS.md and report the result. Do not jump ahead to later phases or stop at a plan.
```

## Continuing prompt

```text
Continue with the next eligible phase from PHASES.md. Inspect PROGRESS.md and the actual code first. Complete the selected phase with tests and a short evidence-based report. Preserve finished work and list any external blockers honestly.
```

## Path and requirement precedence

`SPEC_ROOT` is this extracted directory (recommended `docs/spec/`). All file names in chatbot prompts are relative to it; source/build commands run at the repository root. Keep PROGRESS.md here so its relative phase links stay valid. Implementation evidence goes in repository `docs/VERIFICATION.md` and decision records, not a second progress tracker.

Numbered documents define product behavior; PHASES defines execution order; phase files define local completion checks; document 08 defines final acceptance. If a later implementation decision changes a normative requirement, update the affected documents and record it explicitly. An example or task shorthand cannot silently override a requirement. Read CHANGELOG.md when resuming from version 1.1.

## Version 1.2.1 cross-version checks

Document 10 adds concrete MV01–MV12 scenarios without changing the P01–P14 dependency graph. Read the relevant scenario before implementing its owning phase. P13 must include actual alternating-version runs and observed cache reuse, not merely independent installations of each version.
