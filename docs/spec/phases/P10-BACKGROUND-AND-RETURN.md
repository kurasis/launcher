# P10 — Unload GUI during play and return after exit

**Goal:** Apply the proven lifecycle to the real game so the frontend is disposable while Rust continues supervision.

**Required inputs:** [P03](P03-LIFECYCLE-SPIKE.md), [P09](P09-REAL-GAME-LAUNCH.md)

**Traceability:** Completes the return portion of G3; FR11; A17–A19 and groundwork for A23.

## Read before implementation

- [05-LAUNCH-AND-LIFECYCLE.md](../05-LAUNCH-AND-LIFECYCLE.md)
- [06-UI-AND-IPC.md](../06-UI-AND-IPC.md)

## Tasks

1. Integrate the window controller with production run state and persisted UI selection.
2. Destroy the main webview after successful process startup in default mode; preserve a keep-open preference.
3. Recreate exactly one window on normal exit or crash and recover current state through the snapshot/event protocol.
4. Implement explicit close/quit intent, single-instance reopen, macOS reopen and optional tray actions with a non-tray fallback.
5. Handle early exits and create/destroy races; reconcile stale active-run records after a supervisor restart without killing an unrelated PID.

## Deliverables

- Production background supervisor and automatic return behavior.
- Clear idle, downloading and playing close/quit policies.

## Done when

- [ ] Real Minecraft survives GUI destruction and returns to the same installation after exit.
- [ ] Crash/early exit still restores one usable error view.
- [ ] A second launcher invocation restores the existing supervisor.
- [ ] Idle quit is not trapped; tray absence does not strand the app.

## Demonstration

Play with unload enabled, reopen the GUI once while playing, close it again and exit Minecraft to see automatic restoration.

## Required evidence

Record real lifecycle scenarios on the host; carry untested targets forward to P13.

## Keep out of this phase

Do not add another watchdog executable or claim a fixed background-memory number without measurement.

## Prompt for the development chatbot

```text
Implement P10 — Unload GUI during play and return after exit.
Read START-HERE.md, PHASES.md, phases/P10-BACKGROUND-AND-RETURN.md and its referenced specification sections.
Inspect current code and the single tracker at SPEC_ROOT/PROGRESS.md. Reuse completed work. Implement this phase, not a plan-only response.
Run the meaningful checks in Done when and record exact evidence. Do not replace real integrations with production mocks.
If an external dependency blocks completion, finish independent work in this phase, record the blocker, and do not claim completion.
Update progress and report changed components, checks, demo instructions, remaining blockers and the next eligible phase.
When this request names only this phase, stop after reporting its result.
```

## Additional regression checks

MVP Quit while playing offers Keep background or confirmed Stop game and quit; deliberate detach is deferred. Keep-open mode updates the existing window on exit. Test destruction/exit/reopen races, real crash return, bounded pipe drain and retained redaction state. A surviving orphan blocks shared mutations as well as another launch.

## Version 1.2.1 checks

Cover all background terminal paths: installation success/failure restores one window; cancel-and-exit suppresses restoration; login/pre-spawn close cancels safely. Test bounded restoration without tray support. See [document 10](../10-MULTIVERSION-AND-CACHE.md).
