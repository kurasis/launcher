# P06 — Exact dedicated-server install and supervision

Milestone: **Core**. Initial status: **not_started**.
Dependencies: P03.

## Read first

Read [START-HERE](../START-HERE.md), the current [PROGRESS](../PROGRESS.md), and:

- [05-STORAGE-AND-CONCURRENCY.md](../05-STORAGE-AND-CONCURRENCY.md)
- [06-SERVER-INSTALLATION.md](../06-SERVER-INSTALLATION.md)
- [SOURCES.md](../SOURCES.md)

Additional required contract: [19-LAUNCHER-INTEGRATION.md](../19-LAUNCHER-INTEGRATION.md).

## Work

1. Implement the baseline server adapter, exact source/runtime resolution and verified generation commit.
2. Implement new-world creation, first-use terms, local listener binding, readiness, graceful stop and bounded log drains.
3. Run client and server concurrently through resource/instance locks; add repair and interrupted-install reconciliation.

### Revision 1.0.1 work

Run real server installation/start/stop in isolated qualification mode before production admission exists. Implement any minimal identity bootstrap installation now; proof of actual profile admission remains P07. Defer live pinned-cache repairs.

## Exit criteria

- [ ] The pinned real server starts and stops cleanly with the managed runtime on supported targets.
- [ ] A corrupted managed artifact is detected; repair does not modify a live world or running cache object.
- [ ] Readiness failure, port race, low disk, log sink failure and unresponsive stop are correctly reported.

- [ ] Revision 1.0.1 obligations applicable to this phase are implemented and evidenced; linked regression cases are: A65, A67, A68. Later live-matrix gates remain explicitly pending where this phase only supplies the implementation.

## Evidence and handover

Store phase evidence under `docs/worlds-with-friends/evidence/P06/`. Include exact build/source identities, commands, environment, results, known limitations and whether each result used fixtures or live services. Add meaningful tests for the state transitions, trust boundaries and crash windows changed in this phase. Update generated contracts, migrations, user/operator instructions and [traceability](../TRACEABILITY.md) where affected.

Do not implement unrelated expansion features to bypass a blocker. Reuse qualified earlier facilities and test only the relevant regression surface plus mandatory gates. The exit criteria are all required; a blocked live integration is not a completed phase.

Follow the authorized branch/commit/push/PR/merge workflow in START-HERE. Update PROGRESS with implementation and evidence state before the final phase report. Report in Russian and keep repository artifacts in English.

## Revision 1.0.2 obligations

Provide isolated first-world generation and qualify actual direct-file log sinks. Supply trusted binding-release adapter hooks without claiming the later production admission test has passed.

- [ ] Implement the applicable contract changes and record evidence for A80, A81, A84 at this phase's layer. The primary completion phase remains the one in ACCEPTANCE; later integration/live tests are not falsely marked complete here.
