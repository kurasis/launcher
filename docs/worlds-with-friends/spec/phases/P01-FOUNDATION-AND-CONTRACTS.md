# P01 — Foundation and integration contracts

Milestone: **Core**. Initial status: **not_started**.
Dependencies: none.

## Read first

Read [START-HERE](../START-HERE.md), the current [PROGRESS](../PROGRESS.md), and:

- [02-ARCHITECTURE.md](../02-ARCHITECTURE.md)
- [03-OAUTH-AND-IDENTITY.md](../03-OAUTH-AND-IDENTITY.md)
- [17-INTEGRATION-GATES.md](../17-INTEGRATION-GATES.md)

Additional required contract: [19-LAUNCHER-INTEGRATION.md](../19-LAUNCHER-INTEGRATION.md).

## Work

1. Inventory the real repository, base launcher tests, account service, room API, deployment tooling and existing game identity bridge.
2. Create the feature documentation directory, integration inventory and ADRs. Identify every required change to global activity locks and process ownership.
3. Freeze a provisional core support tuple, runtime flavor, account validation contract, API mapping, migration strategy and qualification plan.
4. Define typed interfaces and schema generation approach. Record unresolved dependency versions as explicit spike decisions, not fake pins.

### Revision 1.0.1 work

Map existing capabilities without the earlier archive. Freeze candidate choices and gate owners, not future spike results. Define trusted pre-play identity hooks and identify any minimal bootstrap required in core.

## Exit criteria

- [ ] Base checks run and results recorded; required missing inputs have named owners and downstream blockers.
- [ ] G01 and the design portion of G04 are resolved; the chosen identity approach has a credible trusted verification point.
- [ ] Architecture and phase scope are reviewable; no implementation or live integration is falsely marked complete.

- [ ] Revision 1.0.1 obligations applicable to this phase are implemented and evidenced; linked regression cases are: A67, A68, A69. Later live-matrix gates remain explicitly pending where this phase only supplies the implementation.

## Evidence and handover

Store phase evidence under `docs/worlds-with-friends/evidence/P01/`. Include exact build/source identities, commands, environment, results, known limitations and whether each result used fixtures or live services. Add meaningful tests for the state transitions, trust boundaries and crash windows changed in this phase. Update generated contracts, migrations, user/operator instructions and [traceability](../TRACEABILITY.md) where affected.

Do not implement unrelated expansion features to bypass a blocker. Reuse qualified earlier facilities and test only the relevant regression surface plus mandatory gates. The exit criteria are all required; a blocked live integration is not a completed phase.

Follow the authorized branch/commit/push/PR/merge workflow in START-HERE. Update PROGRESS with implementation and evidence state before the final phase report. Report in Russian and keep repository artifacts in English.

## Revision 1.0.2 obligations

Map the new cancellation, initialization, identity-release, recovery and privacy-policy capabilities to the actual repository. Record support gaps and a candidate sink/time-service policy; do not demand later live tests in P01.

- [ ] Implement the applicable contract changes and record evidence for A78, A80, A81, A88, A89, A91 at this phase's layer. The primary completion phase remains the one in ACCEPTANCE; later integration/live tests are not falsely marked complete here.
