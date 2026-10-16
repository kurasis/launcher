# P02 — Rust state, storage and typed IPC

**Goal:** Establish the durable backend and a narrow frontend contract that later features can safely use.

**Required inputs:** [P01](P01-PROJECT-FOUNDATION.md)

**Traceability:** G1 foundation; FR05, FR09, FR12, FR14; contributes to A07, A16, A21.

## Read before implementation

- [02-ARCHITECTURE-AND-STORAGE.md](../02-ARCHITECTURE-AND-STORAGE.md)
- [06-UI-AND-IPC.md](../06-UI-AND-IPC.md)
- [07-PACKAGING-AND-SECURITY.md](../07-PACKAGING-AND-SECURITY.md)

## Tasks

1. Implement DATA_ROOT resolution, versioned settings/account-display metadata, atomic JSON writes, backup/recovery and generated instance identifiers.
2. Define Rust application state, typed DTOs, stable error envelopes and the frontend IPC wrapper. Implement get_app_snapshot with backend epoch/revision ordering, buffered bootstrap reconciliation and recoverable terminal outcomes.
3. Implement instance creation, immutable version identity, selected-instance persistence and validated RAM/resolution settings. Until the catalog exists, use an explicit internal compatibility configuration; do not pretend it came from the network.
4. Implement the global activity permit, instance locks, operation/request IDs and idempotency; provide event/snapshot reconciliation. Draft instances have null manifest hashes until P05 resolves them. Preserve upstream JSON bytes outside app schema envelopes.
5. Set restrictive capabilities and production CSP. Create secret-bearing types and central redaction interfaces before auth logging is added.

## Deliverables

- Authoritative Rust application state and typed frontend contracts.
- Persistent settings/instances outside the official Minecraft directory.
- Testable storage and locking primitives.

## Done when

- [ ] Create an instance and restart: state and selection survive.
- [ ] Malformed inputs and unknown schemas are handled without data loss.
- [ ] Interrupted writes preserve a readable prior version.
- [ ] Invalid IDs cannot escape DATA_ROOT, and repeated event subscriptions are cleaned up.

## Demonstration

Create a named installation record, change its memory setting, restart and show the restored values.

## Required evidence

Record storage, input-validation and IPC tests; demonstrate that no token fields are exposed in DTOs.

## Keep out of this phase

No real installation, token handling or game execution yet.

## Prompt for the development chatbot

```text
Implement P02 — Rust state, storage and typed IPC.
Read START-HERE.md, PHASES.md, phases/P02-CORE-STORAGE-AND-IPC.md and its referenced specification sections.
Inspect current code and the single tracker at SPEC_ROOT/PROGRESS.md. Reuse completed work. Implement this phase, not a plan-only response.
Run the meaningful checks in Done when and record exact evidence. Do not replace real integrations with production mocks.
If an external dependency blocks completion, finish independent work in this phase, record the blocker, and do not claim completion.
Update progress and report changed components, checks, demo instructions, remaining blockers and the next eligible phase.
When this request names only this phase, stop after reporting its result.
```

## Version 1.2.1 checks

Implement the OS-backed DATA_ROOT lifetime lock, canonical cache keys, authoritative install-lock generation and separate InstallationHealth. Test a summary/lock mismatch after interrupted commit and two processes contending for the same root. See [document 10](../10-MULTIVERSION-AND-CACHE.md).
