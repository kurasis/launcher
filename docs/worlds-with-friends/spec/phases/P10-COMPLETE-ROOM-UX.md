# P10 — Complete desktop room and hosting experience

Milestone: **Core**. Initial status: **not_started**.
Dependencies: P08, P09.

## Read first

Read [START-HERE](../START-HERE.md), the current [PROGRESS](../PROGRESS.md), and:

- [12-UX.md](../12-UX.md)
- [09-API-AND-IPC.md](../09-API-AND-IPC.md)

Additional required contract: [12-UX.md](../12-UX.md).

## Work

1. Build all core screens with shared visual tokens, keyboard behavior and Russian/English localization.
2. Connect real durable operations and recovery states; distinguish host offline, service outage, full room and unsupported build.
3. Implement close/reopen, explicit quit choices, accessible progress and diagnostic preview/export.

### Revision 1.0.1 work

Show listener/game-start/admission/pre-play states distinctly; display maintenance/reconciliation and privacy reconnect effects. Keep structured and raw debug-log capture separate.

## Exit criteria

- [ ] Keyboard and screen-reader paths cover create, invite, join, stop and restore.
- [ ] No UI event loss causes duplicate work or hides a terminal operation result.
- [ ] A host can close both UI and their game while friends continue; reopening shows actual state.

- [ ] Revision 1.0.1 obligations applicable to this phase are implemented and evidenced; linked regression cases are: A55, A73. Later live-matrix gates remain explicitly pending where this phase only supplies the implementation.

## Evidence and handover

Store phase evidence under `docs/worlds-with-friends/evidence/P10/`. Include exact build/source identities, commands, environment, results, known limitations and whether each result used fixtures or live services. Add meaningful tests for the state transitions, trust boundaries and crash windows changed in this phase. Update generated contracts, migrations, user/operator instructions and [traceability](../TRACEABILITY.md) where affected.

Do not implement unrelated expansion features to bypass a blocker. Reuse qualified earlier facilities and test only the relevant regression surface plus mandatory gates. The exit criteria are all required; a blocked live integration is not a completed phase.

Follow the authorized branch/commit/push/PR/merge workflow in START-HERE. Update PROGRESS with implementation and evidence state before the final phase report. Report in Russian and keep repository artifacts in English.

## Revision 1.0.2 obligations

Show pending cancellation, player cleanup, world initialization and maintenance recovery. Preview restore identity/build constraints and stop-first route-policy changes without repetitive ordinary reconnect prompts.

- [ ] Implement the applicable contract changes and record evidence for A78, A80, A81, A82, A83, A88, A91 at this phase's layer. The primary completion phase remains the one in ACCEPTANCE; later integration/live tests are not falsely marked complete here.
