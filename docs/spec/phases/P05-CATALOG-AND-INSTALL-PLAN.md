# P05 — Minecraft metadata and deterministic install planning

**Goal:** Turn official metadata into a platform-specific GamePlan plus RuntimeRequirement without downloading game binaries yet.

**Required inputs:** [P02](P02-CORE-STORAGE-AND-IPC.md)

**Traceability:** G2 planning; FR04 and part of FR06; A09, groundwork for A10–A11.

## Read before implementation

- [01-PRODUCT-AND-SCOPE.md](../01-PRODUCT-AND-SCOPE.md)
- [02-ARCHITECTURE-AND-STORAGE.md](../02-ARCHITECTURE-AND-STORAGE.md)
- [04-INSTALLATION-AND-SOURCES.md](../04-INSTALLATION-AND-SOURCES.md)
- [09-SOURCES-AND-VERIFICATION.md](../09-SOURCES-AND-VERIFICATION.md)

## Tasks

1. Implement Mojang catalog fetch/cache and the curated compatibility list. Resolve exact version IDs and validate descriptor SHA-1 before parsing.
2. Implement the ordered rules engine, target-platform mapping and feature predicates.
3. Resolve client, applicable libraries/native artifacts, asset index, deduplicated asset objects, logging config and required Java major.
4. Produce a deterministic GamePlan and unresolved RuntimeRequirement with destinations, sizes, expected hashes and source URLs for game artifacts; preserve ordered classpaths and modern native-JAR semantics.
5. Reject unsupported inheritance/legacy formats and unresolved required fields explicitly. Add minimal sanitized fixtures for both baseline releases.

## Deliverables

- Manifest source, rules engine and pure planner producing GamePlan + RuntimeRequirement; P07 resolves RuntimePlan and P08 composes FinalInstallPlan.
- Cached verified metadata and supported-version UI data.
- Per-target plan fixtures for both baseline releases.

## Done when

- [ ] Rules use last matching action, not any-allow semantics.
- [ ] Plans select Java 21/25 from metadata and exclude foreign-OS libraries.
- [ ] Asset hashes are deduplicated; unsafe paths and malformed metadata are rejected.
- [ ] Catalog latest.release never silently upgrades an instance or adds unqualified releases.

## Demonstration

Select a supported version and show a real resolved summary: Java major, game-artifact count and estimated missing game bytes; runtime totals remain unknown until P07 resolves them. Qualification builds may expose only the fixed candidate releases under document 01’s policy.

## Required evidence

Record source checks and plan/rules tests; distinguish metadata success from actual game compatibility.

## Keep out of this phase

No bulk downloads, archive extraction or launching; auth approval is not a dependency of this pure-data phase.

## Prompt for the development chatbot

```text
Implement P05 — Minecraft metadata and deterministic install planning.
Read START-HERE.md, PHASES.md, phases/P05-CATALOG-AND-INSTALL-PLAN.md and its referenced specification sections.
Inspect current code and the single tracker at SPEC_ROOT/PROGRESS.md. Reuse completed work. Implement this phase, not a plan-only response.
Run the meaningful checks in Done when and record exact evidence. Do not replace real integrations with production mocks.
If an external dependency blocks completion, finish independent work in this phase, record the blocker, and do not claim completion.
Update progress and report changed components, checks, demo instructions, remaining blockers and the next eligible phase.
When this request names only this phase, stop after reporting its result.
```

## Version 1.2.1 checks

Preserve classpath order while mapping entries to canonical content identities; test same-coordinate/different-hash and different-coordinate/same-hash cases, conflicting sizes and separate version descriptors. See [document 10](../10-MULTIVERSION-AND-CACHE.md).
