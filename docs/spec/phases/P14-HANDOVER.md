# P14 — Final handover and reproducibility

**Goal:** Deliver an understandable source project and verified packages that another developer or user can operate.

**Required inputs:** [P13](P13-FINAL-QUALIFICATION.md)

**Traceability:** Final confirmation of all gates; A25–A26 and document 08 Definition of done.

## Read before implementation

- [START-HERE.md](../START-HERE.md)
- [07-PACKAGING-AND-SECURITY.md](../07-PACKAGING-AND-SECURITY.md)
- [08-IMPLEMENTATION-AND-ACCEPTANCE.md](../08-IMPLEMENTATION-AND-ACCEPTANCE.md)

## Tasks

1. Finalize developer setup/build instructions, owner auth configuration, user installation, troubleshooting, licenses and known limitations.
2. Verify the exact documented commands from a clean checkout with lockfile enforcement.
3. Remove temporary prototype wiring and stale completion claims; preserve useful tests and decisions.
4. Finalize release notes, compatibility policy, checksums and package references. Keep publishing as a separate authorized action.
5. Update the progress tracker and provide a compact final report: what works, evidence, artifact locations, and any remaining external blockers.

## Deliverables

- Source repository, usable package references and complete handover docs.
- Final verification summary and accurate phase/acceptance status.

## Done when

- [ ] A new developer can build using the guide without undocumented steps.
- [ ] A user can install, sign in, install the game, play and return using the guide.
- [ ] No credentials, downloaded Minecraft binaries or unrelated user data are in the source deliverable.
- [ ] MVP completion is claimed only if P13 and all mandatory gates actually passed.

## Demonstration

Follow the README build/run path and show where a user obtains the correct tested package.

## Required evidence

Record clean-checkout reproduction and the final artifact/verification index.

## Keep out of this phase

Do not add post-MVP features or publish a release as part of documentation cleanup.

## Prompt for the development chatbot

```text
Implement P14 — Final handover and reproducibility.
Read START-HERE.md, PHASES.md, phases/P14-HANDOVER.md and its referenced specification sections.
Inspect current code and the single tracker at SPEC_ROOT/PROGRESS.md. Reuse completed work. Implement this phase, not a plan-only response.
Run the meaningful checks in Done when and record exact evidence. Do not replace real integrations with production mocks.
If an external dependency blocks completion, finish independent work in this phase, record the blocker, and do not claim completion.
Update progress and report changed components, checks, demo instructions, remaining blockers and the next eligible phase.
When this request names only this phase, stop after reporting its result.
```

## Handover consistency check

Ensure docs/spec/PROGRESS.md is the only phase tracker, its relative links resolve, the delivered specification version is recorded, and every final package checksum points to the tested artifact from P13. Do not treat AUDIT-REPORT.md's specification review as evidence that the application has been implemented.
