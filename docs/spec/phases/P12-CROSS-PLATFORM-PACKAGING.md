# P12 — Windows, macOS and Linux packages

**Goal:** Produce installable artifacts and reproducible CI for every claimed target.

**Required inputs:** [P11](P11-COMPLETE-PRODUCT-UI.md)

**Traceability:** G5 packaging; FR13; A01–A02, A24–A26.

## Read before implementation

- [01-PRODUCT-AND-SCOPE.md](../01-PRODUCT-AND-SCOPE.md)
- [07-PACKAGING-AND-SECURITY.md](../07-PACKAGING-AND-SECURITY.md)
- [08-IMPLEMENTATION-AND-ACCEPTANCE.md](../08-IMPLEMENTATION-AND-ACCEPTANCE.md)

## Tasks

1. Build the Windows x64 NSIS installer with WebView2 bootstrap handling and per-user paths.
2. Build native macOS x64/arm64 apps and DMGs; prepare owner-configured signing/notarization without committing keys.
3. Build Linux x64 DEB and AppImage on the qualified glibc/WebKitGTK baseline; verify runtime dependencies and non-tray reopen.
4. Complete the CI matrix and generate artifact checksums/version metadata. Keep signing secrets away from untrusted jobs.
5. Test clean installation, upgrade and uninstall-data preservation. Clearly label unsigned/private artifacts and external signing blockers.

## Deliverables

- Target installers/packages and reproducible build jobs.
- User installation notes and release metadata/checksums.

## Done when

- [ ] A clean supported machine runs the app without Rust/Node/npm or manual Java installation.
- [ ] Missing Windows WebView2 and Linux dependencies are handled/documented accurately.
- [ ] Upgrade preserves settings and worlds; default uninstall preserves user data.
- [ ] Public signing readiness is reported independently from local functional success.

## Demonstration

Install a produced package on a clean target, start the launcher, then verify an upgrade preserves an existing test instance.

## Required evidence

Record artifact paths, hashes, target architecture, signed/unsigned status and tested OS; do not substitute a cross-build for a smoke test.

## Keep out of this phase

No automatic updater, publishing, external deployment or signing-account purchase unless separately authorized.

## Prompt for the development chatbot

```text
Implement P12 — Windows, macOS and Linux packages.
Read START-HERE.md, PHASES.md, phases/P12-CROSS-PLATFORM-PACKAGING.md and its referenced specification sections.
Inspect current code and the single tracker at SPEC_ROOT/PROGRESS.md. Reuse completed work. Implement this phase, not a plan-only response.
Run the meaningful checks in Done when and record exact evidence. Do not replace real integrations with production mocks.
If an external dependency blocks completion, finish independent work in this phase, record the blocker, and do not claim completion.
Update progress and report changed components, checks, demo instructions, remaining blockers and the next eligible phase.
When this request names only this phase, stop after reporting its result.
```

## Qualification package status

These are private qualification packages until P13 finishes. Candidate versions use the explicit qualification-build policy from document 01; normal authentication remains mandatory. Exact OS/build baselines and stable app identity must be recorded. Final package rebuilds after the policy changes are verified in P13; P12 success alone is not a full compatibility claim.
