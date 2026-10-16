# P06 — Verified, cancellable download engine

**Goal:** Reliably fetch planned artifacts with bounded resource usage, integrity checking and recoverable cancellation.

**Required inputs:** [P02](P02-CORE-STORAGE-AND-IPC.md), [P05](P05-CATALOG-AND-INSTALL-PLAN.md)

**Traceability:** G2 transport; FR07; part of A10, A14, A22.

## Read before implementation

- [04-INSTALLATION-AND-SOURCES.md](../04-INSTALLATION-AND-SOURCES.md)
- [06-UI-AND-IPC.md](../06-UI-AND-IPC.md)
- [07-PACKAGING-AND-SECURITY.md](../07-PACKAGING-AND-SECURITY.md)

## Tasks

1. Implement Rust streaming downloads, bounded concurrency, deduplication, staging and atomic promotion.
2. Validate HTTPS source/redirect policies, sizes and expected SHA-1/SHA-256; keep auth and artifact HTTP clients separate.
3. Implement timeouts, bounded retries/backoff, Retry-After, disk checks and explicit cancellation.
4. Publish throttled progress and persist one terminal outcome with replay-safe frontend reconciliation; retain verified files for retry/restart.
5. Add a local fixture HTTP test suite for corruption, short reads, timeout, cancellation and duplicate requests. File-level resume is sufficient.

## Deliverables

- Production DownloadTransport and executor with typed progress/events.
- Deterministic network/integrity tests and restartable job receipts.

## Done when

- [ ] No artifact is committed until size/hash validation succeeds.
- [ ] Cancel/retry reuses verified files and does not mark partial content ready.
- [ ] Logs contain no credentials; redirects cannot escape the source policy.
- [ ] Queue/buffer/progress rates stay bounded; transient errors do not retry forever.

## Demonstration

Download a small planned artifact set, interrupt it, retry, then demonstrate checksum rejection with a test fixture.

## Required evidence

Record fixture cases and a small public-source smoke test; avoid unnecessary full game downloads during iteration.

## Keep out of this phase

Byte-range resume is optional; do not add a generic frontend download API or a custom CDN.

## Prompt for the development chatbot

```text
Implement P06 — Verified, cancellable download engine.
Read START-HERE.md, PHASES.md, phases/P06-DOWNLOAD-ENGINE.md and its referenced specification sections.
Inspect current code and the single tracker at SPEC_ROOT/PROGRESS.md. Reuse completed work. Implement this phase, not a plan-only response.
Run the meaningful checks in Done when and record exact evidence. Do not replace real integrations with production mocks.
If an external dependency blocks completion, finish independent work in this phase, record the blocker, and do not claim completion.
Update progress and report changed components, checks, demo instructions, remaining blockers and the next eligible phase.
When this request names only this phase, stop after reporting its result.
```

## Version 1.2.1 checks

Measure unique network transfers and reuse existing verified objects across instances. Give partial files job-owned paths, reject conflicting metadata and keep progress bounded across retries. See [document 10](../10-MULTIVERSION-AND-CACHE.md).
