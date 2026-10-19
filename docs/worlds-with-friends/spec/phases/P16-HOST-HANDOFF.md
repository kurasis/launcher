# P16 — Verified host handoff and conflict control

Milestone: **Expansion**. Initial status: **not_started**.
Dependencies: P15, P09.

## Read first

Read [START-HERE](../START-HERE.md), the current [PROGRESS](../PROGRESS.md), and:

- [16-ADVANCED-HOSTING.md](../16-ADVANCED-HOSTING.md)
- [11-WORLDS-AND-BACKUPS.md](../11-WORLDS-AND-BACKUPS.md)
- [04-ROOMS-AND-PERMISSIONS.md](../04-ROOMS-AND-PERMISSIONS.md)

Additional required contract: [18-AUTHORITY-AND-MAINTENANCE.md](../18-AUTHORITY-AND-MAINTENANCE.md).

## Work

1. Implement durable handoff state and source stop/snapshot/transfer/target verification.
2. Coordinate epoch transition, source surrender, target start and client rejoin.
3. Implement failure recovery, blocked authority and explicit fork behavior without silent world merging.

### Revision 1.0.1 work

Use a renewable handoff maintenance operation after source gameplay has stopped. Verify both receipts, commit placement once, and reconcile a lost commit response without restarting both hosts.

## Exit criteria

- [ ] The target resumes the verified world and source cannot reopen room admissions under an old epoch.
- [ ] Partitions at every step do not create two accepted authoritative histories.
- [ ] Target start failure preserves recoverable source and snapshot data.

- [ ] Revision 1.0.1 obligations applicable to this phase are implemented and evidenced; linked regression cases are: A49, A77. Later live-matrix gates remain explicitly pending where this phase only supplies the implementation.

## Evidence and handover

Store phase evidence under `docs/worlds-with-friends/evidence/P16/`. Include exact build/source identities, commands, environment, results, known limitations and whether each result used fixtures or live services. Add meaningful tests for the state transitions, trust boundaries and crash windows changed in this phase. Update generated contracts, migrations, user/operator instructions and [traceability](../TRACEABILITY.md) where affected.

Do not implement unrelated expansion features to bypass a blocker. Reuse qualified earlier facilities and test only the relevant regression surface plus mandatory gates. The exit criteria are all required; a blocked live integration is not a completed phase.

Follow the authorized branch/commit/push/PR/merge workflow in START-HERE. Update PROGRESS with implementation and evidence state before the final phase report. Report in Russian and keep repository artifacts in English.

## Revision 1.0.2 obligations

Enforce unique world custody across rooms/devices and fenced recovery for interrupted handoffs. A replacement recovery worker does not obtain new source/target authority implicitly.

- [ ] Implement the applicable contract changes and record evidence for A87, A88 at this phase's layer. The primary completion phase remains the one in ACCEPTANCE; later integration/live tests are not falsely marked complete here.
