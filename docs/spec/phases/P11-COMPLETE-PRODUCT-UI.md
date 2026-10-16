# P11 — Complete web UI, settings and diagnostics

**Goal:** Finish a coherent, redesignable React interface backed by real operations.

**Required inputs:** [P04](P04-AUTHENTICATION-AND-ACCESS.md), [P08](P08-COMPLETE-GAME-INSTALLER.md), [P10](P10-BACKGROUND-AND-RETURN.md)

**Traceability:** G4; FR09, FR14, FR16 plus all UI surfaces; A07, A21–A22.

## Read before implementation

- [01-PRODUCT-AND-SCOPE.md](../01-PRODUCT-AND-SCOPE.md)
- [06-UI-AND-IPC.md](../06-UI-AND-IPC.md)
- [07-PACKAGING-AND-SECURITY.md](../07-PACKAGING-AND-SECURITY.md)

## Tasks

1. Complete installations, create/details, account, downloads, settings, diagnostics and About screens.
2. Centralize visual tokens, reusable components and externalized strings; support system/dark/light themes and reduced motion.
3. Wire every button to typed backend commands and show accurate busy, cancelled, failed, no-access and retry states.
4. Implement open-instance-folder and user-requested redacted diagnostic export with a constrained file list.
5. Verify keyboard navigation, focus, accessible labels, minimum window size and enlarged text; remove the isolated prototype controls from production.

## Deliverables

- Complete real UI with reusable styling/component structure.
- Useful error recovery and sanitized diagnostic export.

## Done when

- [ ] The entire first-use journey works without a terminal or fake backend responses.
- [ ] A visual theme/layout change does not require modifying auth/install/launch modules.
- [ ] Keyboard users can operate primary flows and dismiss dialogs correctly.
- [ ] Exports exclude tokens, argfiles, account secrets and worlds.

## Demonstration

Walk through first use, cancellation/retry, settings, a game run and a diagnostic export using only the UI.

## Required evidence

Record UI checks and screenshots if useful; screenshots do not replace functional evidence.

## Keep out of this phase

No social feed, mods catalog, remote scripts, copied artwork or speculative UI features outside MVP.

## Prompt for the development chatbot

```text
Implement P11 — Complete web UI, settings and diagnostics.
Read START-HERE.md, PHASES.md, phases/P11-COMPLETE-PRODUCT-UI.md and its referenced specification sections.
Inspect current code and the single tracker at SPEC_ROOT/PROGRESS.md. Reuse completed work. Implement this phase, not a plan-only response.
Run the meaningful checks in Done when and record exact evidence. Do not replace real integrations with production mocks.
If an external dependency blocks completion, finish independent work in this phase, record the blocker, and do not claim completion.
Update progress and report changed components, checks, demo instructions, remaining blockers and the next eligible phase.
When this request names only this phase, stop after reporting its result.
```

## Additional regression checks

Expose one during-game setting with unload-and-return or keep-open; do not add a contradictory auto-reopen toggle. Handle global APP_BUSY across instances, terminal outcomes recovered from snapshots, epoch changes, and sign-out-pending vault errors. Validate concrete RAM/resolution bounds in Rust and reflect the same rules in UI.

## Version 1.2.1 checks

Show InstallationHealth separately from operation progress, wire cancellation by operation kind, and report logging-degraded or unsupported-root states without misleading success. See [document 10](../10-MULTIVERSION-AND-CACHE.md).
