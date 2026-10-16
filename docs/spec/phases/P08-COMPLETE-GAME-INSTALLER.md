# P08 — Complete installation, recovery and repair

**Goal:** Create a complete, verified game installation from the plan and keep user-owned game data safe during repair.

**Required inputs:** [P05](P05-CATALOG-AND-INSTALL-PLAN.md), [P06](P06-DOWNLOAD-ENGINE.md), [P07](P07-MANAGED-JAVA.md)

**Traceability:** Completes G2; FR06–FR08; A10, A14–A15 and part of A22.

## Read before implementation

- [02-ARCHITECTURE-AND-STORAGE.md](../02-ARCHITECTURE-AND-STORAGE.md)
- [04-INSTALLATION-AND-SOURCES.md](../04-INSTALLATION-AND-SOURCES.md)
- [06-UI-AND-IPC.md](../06-UI-AND-IPC.md)

## Tasks

1. Compose GamePlan + RuntimePlan into FinalInstallPlan; orchestrate file checking, downloader and runtime installation through the documented install states.
2. Install client, libraries, asset objects/index projection and logging configuration into the specified layout.
3. Commit install.lock.json only after the full plan succeeds; enforce the global activity permit plus per-instance/shared-artifact locks in the documented order.
4. Implement file-level retry/restart and full-hash Repair while preserving saves/options and all user-owned data.
5. Connect a minimal real Install/Cancel/Retry/Repair view; postpone visual polish.

## Deliverables

- End-to-end vanilla installer and repair flow.
- A ready baseline instance with a complete lock and reusable shared artifacts.

## Done when

- [ ] Fresh installation finishes with all planned artifacts verified.
- [ ] A cancelled/crashed install resumes without downloading valid completed files again.
- [ ] Repair replaces one corrupted artifact and leaves sentinel world/settings files byte-identical.
- [ ] No install/repair mutates shared content during any game; same-ID/different-hash asset indexes project correctly between runs. Cancellation races with commit report the actual terminal state.

## Demonstration

Install 1.21.1, repair a deliberately corrupted managed file, and show that a dummy world/settings file is unchanged.

## Required evidence

Record install receipts, integrity/recovery tests and actual transfer totals without distributing game artifacts.

## Keep out of this phase

No game launch yet and no implicit claim that ready files prove account entitlement.

## Prompt for the development chatbot

```text
Implement P08 — Complete installation, recovery and repair.
Read START-HERE.md, PHASES.md, phases/P08-COMPLETE-GAME-INSTALLER.md and its referenced specification sections.
Inspect current code and the single tracker at SPEC_ROOT/PROGRESS.md. Reuse completed work. Implement this phase, not a plan-only response.
Run the meaningful checks in Done when and record exact evidence. Do not replace real integrations with production mocks.
If an external dependency blocks completion, finish independent work in this phase, record the blocker, and do not claim completion.
Update progress and report changed components, checks, demo instructions, remaining blockers and the next eligible phase.
When this request names only this phase, stop after reporting its result.
```

## Version 1.2.1 checks

Freeze the resolved job plan before downloads, commit the install lock before its derived instance summary, invalidate health for all users of a corrupted shared object, and verify the MV recovery scenarios. See [document 10](../10-MULTIVERSION-AND-CACHE.md).
