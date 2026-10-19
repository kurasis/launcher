# P13 — Modpack generations and first Fabric room

Milestone: **Expansion**. Initial status: **not_started**.
Dependencies: P12.

## Read first

Read [START-HERE](../START-HERE.md), the current [PROGRESS](../PROGRESS.md), and:

- [10-PACK-SYNCHRONIZATION.md](../10-PACK-SYNCHRONIZATION.md)
- [06-SERVER-INSTALLATION.md](../06-SERVER-INSTALLATION.md)
- [11-WORLDS-AND-BACKUPS.md](../11-WORLDS-AND-BACKUPS.md)

Additional required contract: [10-PACK-SYNCHRONIZATION.md](../10-PACK-SYNCHRONIZATION.md).

## Work

1. Implement immutable client/server manifests, provider resolution and explicit side/dependency classification.
2. Qualify one pinned Fabric tuple including the project identity bridge and required runtime.
3. Implement staged pack updates, configuration conflicts and rollback with the matching world snapshot.

### Revision 1.0.1 work

Use detached manifest hashes and per-target installation locks. Test candidate migrations from immutable snapshots, then revalidate final data and commit world/pack together.

## Exit criteria

- [ ] A clean guest installs and joins the tested modded room without copying client-only files into the server.
- [ ] Restricted/unknown redistribution and executable consent are enforced.
- [ ] A failed world migration rolls back the pack plus matching snapshot, not just JAR files.

- [ ] Revision 1.0.1 obligations applicable to this phase are implemented and evidenced; linked regression cases are: A45, A66. Later live-matrix gates remain explicitly pending where this phase only supplies the implementation.

## Evidence and handover

Store phase evidence under `docs/worlds-with-friends/evidence/P13/`. Include exact build/source identities, commands, environment, results, known limitations and whether each result used fixtures or live services. Add meaningful tests for the state transitions, trust boundaries and crash windows changed in this phase. Update generated contracts, migrations, user/operator instructions and [traceability](../TRACEABILITY.md) where affected.

Do not implement unrelated expansion features to bypass a blocker. Reuse qualified earlier facilities and test only the relevant regression surface plus mandatory gates. The exit criteria are all required; a blocked live integration is not a completed phase.

Follow the authorized branch/commit/push/PR/merge workflow in START-HERE. Update PROGRESS with implementation and evidence state before the final phase report. Report in Russian and keep repository artifacts in English.

## Revision 1.0.2 obligations

Apply exact snapshot pack/player mapping on update and rollback; repeat direct-file sink audits and native download-origin tests for supported pack tuples.

- [ ] Implement the applicable contract changes and record evidence for A83, A84, A85 at this phase's layer. The primary completion phase remains the one in ACCEPTANCE; later integration/live tests are not falsely marked complete here.
