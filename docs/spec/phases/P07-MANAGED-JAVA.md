# P07 — Private Java runtime installation

**Goal:** Install the exact compatible Java major and architecture without changing the user’s system Java.

**Required inputs:** [P05](P05-CATALOG-AND-INSTALL-PLAN.md), [P06](P06-DOWNLOAD-ENGINE.md)

**Traceability:** G2 runtime; FR06; A11 and part of A10/A16.

## Read before implementation

- [02-ARCHITECTURE-AND-STORAGE.md](../02-ARCHITECTURE-AND-STORAGE.md)
- [04-INSTALLATION-AND-SOURCES.md](../04-INSTALLATION-AND-SOURCES.md)
- [09-SOURCES-AND-VERIFICATION.md](../09-SOURCES-AND-VERIFICATION.md)

## Tasks

1. Implement the Adoptium provider using descriptor major plus target OS/architecture; resolve a GA portable JRE and documented same-major JDK fallback.
2. Pin the returned release/package metadata in a receipt, download through P06 and verify SHA-256.
3. Implement safe ZIP/tar extraction, internal-link handling, size limits, execute permissions and staging promotion.
4. Build and validate the extracted-tree receipt before finding/executing Java. Validate its reported major/architecture with sanitized environment and bounded capture. Repair the pinned build from fresh staging if any tree entry is corrupt.
5. Cache completed runtimes and retain their notices; never modify PATH, JAVA_HOME, registry associations or the official launcher installation.

## Deliverables

- RuntimeProvider, safe runtime extractor and immutable runtime receipts.
- Tested Java executable discovery on each available target.

## Done when

- [ ] The baseline descriptors resolve the correct Java major.
- [ ] A package with an escaping link or invalid hash cannot execute; corruption of extracted Java is detected before a probe, and required safe internal links work on qualified packages.
- [ ] Installed Java -version succeeds and matches the target.
- [ ] A second install reuses the verified runtime; a missing platform package produces an explicit error.

## Demonstration

Install a managed runtime on the host, show its verified version/path, then resolve a different target without executing it.

## Required evidence

Record actual archive digest checks and execution separately from metadata-only platform tests.

## Keep out of this phase

No global installer, arbitrary runtime picker or silent fallback to a different Java major/architecture.

## Prompt for the development chatbot

```text
Implement P07 — Private Java runtime installation.
Read START-HERE.md, PHASES.md, phases/P07-MANAGED-JAVA.md and its referenced specification sections.
Inspect current code and the single tracker at SPEC_ROOT/PROGRESS.md. Reuse completed work. Implement this phase, not a plan-only response.
Run the meaningful checks in Done when and record exact evidence. Do not replace real integrations with production mocks.
If an external dependency blocks completion, finish independent work in this phase, record the blocker, and do not claim completion.
Update progress and report changed components, checks, demo instructions, remaining blockers and the next eligible phase.
When this request names only this phase, stop after reporting its result.
```

## Version 1.2.1 checks

Key runtime storage by OS/architecture/major/image-type/archive digest. Test JRE and JDK with identical release labels plus two distinct package hashes; never overwrite one with the other. See [document 10](../10-MULTIVERSION-AND-CACHE.md).
