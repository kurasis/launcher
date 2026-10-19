# P09 — World import, verified snapshots and restore

Milestone: **Core**. Initial status: **not_started**.
Dependencies: P06, P08.

## Read first

Read [START-HERE](../START-HERE.md), the current [PROGRESS](../PROGRESS.md), and:

- [11-WORLDS-AND-BACKUPS.md](../11-WORLDS-AND-BACKUPS.md)
- [05-STORAGE-AND-CONCURRENCY.md](../05-STORAGE-AND-CONCURRENCY.md)
- [13-SECURITY.md](../13-SECURITY.md)

Additional required contract: [18-AUTHORITY-AND-MAINTENANCE.md](../18-AUTHORITY-AND-MAINTENANCE.md).

## Work

1. Implement copied import, world lineage, mutable-data policies and stopped-world snapshot creation.
2. Implement manifest verification, retention, candidate restore, validation launch and atomic world pointer change.
3. Add export with secret filtering and explicit deletion/custody semantics.

### Revision 1.0.1 work

Implement stable-source import, actor-owned lock transition, disposable validation clone and backend/local commit reconciliation. World candidate bytes must not be changed by validation.

## Exit criteria

- [ ] A known world restores with verified player inventory, dimensions and expected gameplay data.
- [ ] Corrupt/incomplete snapshots and traversal archives fail without touching the active world.
- [ ] Crash at each publication/pointer boundary retains an unambiguous recoverable state.

- [ ] Revision 1.0.1 obligations applicable to this phase are implemented and evidenced; linked regression cases are: A60, A61, A62, A63, A64. Later live-matrix gates remain explicitly pending where this phase only supplies the implementation.

## Evidence and handover

Store phase evidence under `docs/worlds-with-friends/evidence/P09/`. Include exact build/source identities, commands, environment, results, known limitations and whether each result used fixtures or live services. Add meaningful tests for the state transitions, trust boundaries and crash windows changed in this phase. Update generated contracts, migrations, user/operator instructions and [traceability](../TRACEABILITY.md) where affected.

Do not implement unrelated expansion features to bypass a blocker. Reuse qualified earlier facilities and test only the relevant regression surface plus mandatory gates. The exit criteria are all required; a blocked live integration is not a completed phase.

Follow the authorized branch/commit/push/PR/merge workflow in START-HERE. Update PROGRESS with implementation and evidence state before the final phase report. Report in Russian and keep repository artifacts in English.

## Revision 1.0.2 obligations

Implement non-pointer result receipts and export disclosure authorization, snapshot-bound build/player mapping, fenced recovery claims and deletion-safe reconciliation. Test clone space on filesystems without reflinks.

- [ ] Implement the applicable contract changes and record evidence for A82, A83, A88, A90 at this phase's layer. The primary completion phase remains the one in ACCEPTANCE; later integration/live tests are not falsely marked complete here.
