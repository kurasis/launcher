# P14 — NeoForge, Forge and Quilt qualification

Milestone: **Expansion**. Initial status: **not_started**.
Dependencies: P13.

## Read first

Read [START-HERE](../START-HERE.md), the current [PROGRESS](../PROGRESS.md), and:

- [06-SERVER-INSTALLATION.md](../06-SERVER-INSTALLATION.md)
- [10-PACK-SYNCHRONIZATION.md](../10-PACK-SYNCHRONIZATION.md)
- [SOURCES.md](../SOURCES.md)

Additional required contract: [19-LAUNCHER-INTEGRATION.md](../19-LAUNCHER-INTEGRATION.md).

## Work

1. Implement separate resolver/installer/launch adapters for selected NeoForge, Forge and Quilt tuples.
2. Preserve processor outputs and generated arguments; validate client/server pairing and identity capability.
3. Publish a per-tuple matrix with explicit exclusions rather than one global compatibility badge.

### Revision 1.0.1 work

Distinguish any core-required bootstrap from general advertised loader coverage. Each newly advertised tuple still needs complete actual play/restore and identity qualification.

## Exit criteria

- [ ] Every advertised loader has a real two-account play/save/restore test on its declared targets.
- [ ] Unsupported combinations are rejected before installation where determinable.
- [ ] Source metadata and installer outputs are pinned and reproducible.

- [ ] Revision 1.0.1 obligations applicable to this phase are implemented and evidenced; linked regression cases are: A47, A68. Later live-matrix gates remain explicitly pending where this phase only supplies the implementation.

## Evidence and handover

Store phase evidence under `docs/worlds-with-friends/evidence/P14/`. Include exact build/source identities, commands, environment, results, known limitations and whether each result used fixtures or live services. Add meaningful tests for the state transitions, trust boundaries and crash windows changed in this phase. Update generated contracts, migrations, user/operator instructions and [traceability](../TRACEABILITY.md) where affected.

Do not implement unrelated expansion features to bypass a blocker. Reuse qualified earlier facilities and test only the relevant regression surface plus mandatory gates. The exit criteria are all required; a blocked live integration is not a completed phase.

Follow the authorized branch/commit/push/PR/merge workflow in START-HERE. Update PROGRESS with implementation and evidence state before the final phase report. Report in Russian and keep repository artifacts in English.

## Revision 1.0.2 obligations

Repeat player teardown, persistent mapping, actual file-output and source-policy qualification for every newly advertised loader tuple.

- [ ] Implement the applicable contract changes and record evidence for A80, A83, A84, A85 at this phase's layer. The primary completion phase remains the one in ACCEPTANCE; later integration/live tests are not falsely marked complete here.
