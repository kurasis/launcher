# P03 — Early window and process lifecycle proof

**Goal:** Prove that the chosen Tauri architecture can unload the GUI and keep supervising a process before investing in the launcher.

**Required inputs:** [P02](P02-CORE-STORAGE-AND-IPC.md)

**Traceability:** Early G1 lifecycle proof; groundwork for FR11 and A17–A19.

## Read before implementation

- [05-LAUNCH-AND-LIFECYCLE.md](../05-LAUNCH-AND-LIFECYCLE.md)
- [06-UI-AND-IPC.md](../06-UI-AND-IPC.md)
- [07-PACKAGING-AND-SECURITY.md](../07-PACKAGING-AND-SECURITY.md)

## Tasks

1. Create an isolated test harness using a harmless known test child, never a fake Minecraft launch in production.
2. Keep the child handle and job state in Rust; destroy the main WebviewWindow while it runs, intercept only the required app-exit path and recreate the window on child exit.
3. Implement the single-instance/reopen path and basic idle-quit versus background-keepalive intent.
4. Test immediate child exit, reopening during execution, and exit during window destruction. Serialize window operations.
5. Exercise the spike on available target systems and record missing systems explicitly; final matrix qualification occurs in P13.

## Deliverables

- A small lifecycle test harness and reusable window controller.
- A decision record confirming the keepalive/recreation approach.

## Done when

- [ ] Destroying the GUI does not terminate the test child or Rust supervisor.
- [ ] Child exit restores exactly one usable window.
- [ ] Reopening the app cannot create a second supervisor.
- [ ] Idle Quit still exits normally; test controls cannot launch arbitrary processes from the release UI.

## Demonstration

Run a short test child, watch the GUI disappear, then return automatically with its recorded result.

## Required evidence

Record lifecycle scenarios per OS; mark unavailable systems unverified rather than claiming cross-platform success.

## Keep out of this phase

No real Minecraft launch, production readiness claims or elaborate tray UI.

## Prompt for the development chatbot

```text
Implement P03 — Early window and process lifecycle proof.
Read START-HERE.md, PHASES.md, phases/P03-LIFECYCLE-SPIKE.md and its referenced specification sections.
Inspect current code and the single tracker at SPEC_ROOT/PROGRESS.md. Reuse completed work. Implement this phase, not a plan-only response.
Run the meaningful checks in Done when and record exact evidence. Do not replace real integrations with production mocks.
If an external dependency blocks completion, finish independent work in this phase, record the blocker, and do not claim completion.
Update progress and report changed components, checks, demo instructions, remaining blockers and the next eligible phase.
When this request names only this phase, stop after reporting its result.
```

## Additional regression checks

Test native menu/keyboard Quit as well as window Close; no inference from exit code alone. Test output pipes with inherited handles and bounded post-exit drain. Document the pinned Tauri APIs used and verify that a delayed window destroy cannot run after restoration.
