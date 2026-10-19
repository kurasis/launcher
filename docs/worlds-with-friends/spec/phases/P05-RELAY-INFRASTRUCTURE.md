# P05 — Private relay infrastructure and access enforcement

Milestone: **Core**. Initial status: **not_started**.
Dependencies: P04.

## Read first

Read [START-HERE](../START-HERE.md), the current [PROGRESS](../PROGRESS.md), and:

- [08-RELAY-OPERATIONS.md](../08-RELAY-OPERATIONS.md)
- [13-SECURITY.md](../13-SECURITY.md)
- [15-DEPLOYMENT.md](../15-DEPLOYMENT.md)

Additional required contract: [08-RELAY-OPERATIONS.md](../08-RELAY-OPERATIONS.md).

## Work

1. Deploy a private test relay through reviewed templates with TLS, account/device allow policy and bounded limits.
2. Integrate expiring endpoint permissions, active revocation, metering, health checks and metrics.
3. Prepare second-node discovery/drain configuration and explicit provider traffic accounting.

### Revision 1.0.1 work

Enforce active allow-record expiry, conservative renewal deadlines and server-authorized context attribution. A connect-time callback alone does not satisfy revocation.

## Exit criteria

- [ ] Unregistered and expired devices cannot use the relay; active revoke closes access within the declared contract.
- [ ] A real external network can use the private relay; local mocks do not substitute for G06.
- [ ] Rate/capacity limits, TLS failure and policy service outage have observed results.

- [ ] Revision 1.0.1 obligations applicable to this phase are implemented and evidenced; linked regression cases are: A56, A58. Later live-matrix gates remain explicitly pending where this phase only supplies the implementation.

## Evidence and handover

Store phase evidence under `docs/worlds-with-friends/evidence/P05/`. Include exact build/source identities, commands, environment, results, known limitations and whether each result used fixtures or live services. Add meaningful tests for the state transitions, trust boundaries and crash windows changed in this phase. Update generated contracts, migrations, user/operator instructions and [traceability](../TRACEABILITY.md) where affected.

Do not implement unrelated expansion features to bypass a blocker. Reuse qualified earlier facilities and test only the relevant regression surface plus mandatory gates. The exit criteria are all required; a blocked live integration is not a completed phase.

Follow the authorized branch/commit/push/PR/merge workflow in START-HERE. Update PROGRESS with implementation and evidence state before the final phase report. Report in Russian and keep repository artifacts in English.

## Revision 1.0.2 obligations

Use bounded policy-request deadlines and independently responsive expiry watchdogs during saturated queues; qualify fail-closed clock/renewal behavior.

- [ ] Implement the applicable contract changes and record evidence for A89 at this phase's layer. The primary completion phase remains the one in ACCEPTANCE; later integration/live tests are not falsely marked complete here.
