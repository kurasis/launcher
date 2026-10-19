# P15 — Remote home agent and delegated start

Milestone: **Expansion**. Initial status: **not_started**.
Dependencies: P12.

## Read first

Read [START-HERE](../START-HERE.md), the current [PROGRESS](../PROGRESS.md), and:

- [16-ADVANCED-HOSTING.md](../16-ADVANCED-HOSTING.md)
- [03-OAUTH-AND-IDENTITY.md](../03-OAUTH-AND-IDENTITY.md)
- [15-DEPLOYMENT.md](../15-DEPLOYMENT.md)

Additional required contract: [18-AUTHORITY-AND-MAINTENANCE.md](../18-AUTHORITY-AND-MAINTENANCE.md).

## Work

1. Package a qualified headless host agent with owner-approved public-key enrollment.
2. Implement per-device allowed rooms, command capabilities, resource ceilings and schedules.
3. Provide delegated start/stop UI and explicit unavailable/sleeping behavior.

### Revision 1.0.1 work

Activate remote execution only with explicit device consent bound to current ownerRevision. Use scoped endpoints and maintenance operations rather than arbitrary commands.

## Exit criteria

- [ ] A friend can start only an authorized room on an already reachable enrolled host.
- [ ] Pairing replay, wrong owner, stale commands and device revocation tests pass.
- [ ] Reboot/service lifecycle and privilege boundaries are documented and tested.

- [ ] Revision 1.0.1 obligations applicable to this phase are implemented and evidenced; linked regression cases are: A48, A71. Later live-matrix gates remain explicitly pending where this phase only supplies the implementation.

## Evidence and handover

Store phase evidence under `docs/worlds-with-friends/evidence/P15/`. Include exact build/source identities, commands, environment, results, known limitations and whether each result used fixtures or live services. Add meaningful tests for the state transitions, trust boundaries and crash windows changed in this phase. Update generated contracts, migrations, user/operator instructions and [traceability](../TRACEABILITY.md) where affected.

Do not implement unrelated expansion features to bypass a blocker. Reuse qualified earlier facilities and test only the relevant regression surface plus mandatory gates. The exit criteria are all required; a blocked live integration is not a completed phase.

Follow the authorized branch/commit/push/PR/merge workflow in START-HERE. Update PROGRESS with implementation and evidence state before the final phase report. Report in Russian and keep repository artifacts in English.

## Revision 1.0.2 obligations

Carry cancellation/execution fences and recovery-worker identity into remote operations; a delayed remote start must not defeat an explicit stop.

- [ ] Implement the applicable contract changes and record evidence for A78, A88 at this phase's layer. The primary completion phase remains the one in ACCEPTANCE; later integration/live tests are not falsely marked complete here.
