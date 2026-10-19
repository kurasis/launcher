# P08 — Isolated profiles and invitation-to-game flow

Milestone: **Core**. Initial status: **not_started**.
Dependencies: P07.

## Read first

Read [START-HERE](../START-HERE.md), the current [PROGRESS](../PROGRESS.md), and:

- [10-PACK-SYNCHRONIZATION.md](../10-PACK-SYNCHRONIZATION.md)
- [12-UX.md](../12-UX.md)
- [09-API-AND-IPC.md](../09-API-AND-IPC.md)

Additional required contract: [07-TUNNEL-PROTOCOL.md](../07-TUNNEL-PROTOCOL.md).

## Work

1. Implement invitation preview/redemption, exact baseline profile preparation and client launch orchestration.
2. Recheck generation/epoch after downloads and before admission; preserve user settings in separate locations.
3. Connect the supported client automatically or expose the tested version-specific fallback; implement cancellation boundary.

### Revision 1.0.1 work

Arm the local listener before game launch; mint tickets on actual gameplay connection. Handle native status locally from authorized metadata. Do not consume a slot throughout Java loading.

## Exit criteria

- [ ] A clean guest device prepares the correct build and joins from an invitation.
- [ ] A room update during download cannot launch a stale profile; repeat clicks do not spawn duplicates.
- [ ] Unrelated user instances remain unchanged and new executable content requires appropriate consent.

- [ ] Revision 1.0.1 obligations applicable to this phase are implemented and evidenced; linked regression cases are: A53, A55, A66. Later live-matrix gates remain explicitly pending where this phase only supplies the implementation.

## Evidence and handover

Store phase evidence under `docs/worlds-with-friends/evidence/P08/`. Include exact build/source identities, commands, environment, results, known limitations and whether each result used fixtures or live services. Add meaningful tests for the state transitions, trust boundaries and crash windows changed in this phase. Update generated contracts, migrations, user/operator instructions and [traceability](../TRACEABILITY.md) where affected.

Do not implement unrelated expansion features to bypass a blocker. Reuse qualified earlier facilities and test only the relevant regression surface plus mandatory gates. The exit criteria are all required; a blocked live integration is not a completed phase.

Follow the authorized branch/commit/push/PR/merge workflow in START-HERE. Update PROGRESS with implementation and evidence state before the final phase report. Report in Russian and keep repository artifacts in English.

## Revision 1.0.2 obligations

Preserve initial gameplay bytes through local classification, bind listener lifetime to the exact client run/account/session, enforce caps and close stale listeners. Propagate precise cancellation and cleanup states.

- [ ] Implement the applicable contract changes and record evidence for A78, A80, A85, A86 at this phase's layer. The primary completion phase remains the one in ACCEPTANCE; later integration/live tests are not falsely marked complete here.
