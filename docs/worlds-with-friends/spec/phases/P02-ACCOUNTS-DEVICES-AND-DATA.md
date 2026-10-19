# P02 — Own accounts, device registration and data model

Milestone: **Core**. Initial status: **not_started**.
Dependencies: P01.

## Read first

Read [START-HERE](../START-HERE.md), the current [PROGRESS](../PROGRESS.md), and:

- [03-OAUTH-AND-IDENTITY.md](../03-OAUTH-AND-IDENTITY.md)
- [04-ROOMS-AND-PERMISSIONS.md](../04-ROOMS-AND-PERMISSIONS.md)
- [09-API-AND-IPC.md](../09-API-AND-IPC.md)

Additional required contract: [18-AUTHORITY-AND-MAINTENANCE.md](../18-AUTHORITY-AND-MAINTENANCE.md).

## Work

1. Integrate own OAuth browser login, secure token validation/storage, refresh serialization and sign-out generations.
2. Implement or map canonical accounts, devices, room/session models, migrations, unique constraints and transactional outbox.
3. Implement registration challenges and device key proof; generate API/DTO contracts and establish error envelopes.

### Revision 1.0.1 work

Implement account/device libraries and schemas in a native headless test harness, then hand production ownership to P03. Include stable control keys, scoped endpoint binding schema, proof-specific validation, secret-safe idempotency, maintenance records and authorityEpoch.

## Exit criteria

- [ ] A real own-provider login and refresh succeeds; wrong issuer/audience/state and revoked credentials fail.
- [ ] Device spoofing/replay and concurrent registration are rejected; migration upgrade and restore tests pass.
- [ ] The backend mapping to any existing room service is documented and contract-tested.

- [ ] Revision 1.0.1 obligations applicable to this phase are implemented and evidenced; linked regression cases are: A70, A76. Later live-matrix gates remain explicitly pending where this phase only supplies the implementation.

## Evidence and handover

Store phase evidence under `docs/worlds-with-friends/evidence/P02/`. Include exact build/source identities, commands, environment, results, known limitations and whether each result used fixtures or live services. Add meaningful tests for the state transitions, trust boundaries and crash windows changed in this phase. Update generated contracts, migrations, user/operator instructions and [traceability](../TRACEABILITY.md) where affected.

Do not implement unrelated expansion features to bypass a blocker. Reuse qualified earlier facilities and test only the relevant regression surface plus mandatory gates. The exit criteria are all required; a blocked live integration is not a completed phase.

Follow the authorized branch/commit/push/PR/merge workflow in START-HERE. Update PROGRESS with implementation and evidence state before the final phase report. Report in Russian and keep repository artifacts in English.

## Revision 1.0.2 obligations

Generate durable operation/recovery schemas, world-reference constraints, execution/route revisions, retry-disclosure authorization and protected recovery-retention contracts. Test selected clock and idempotency primitives; production room workflows complete in P07.

- [ ] Implement the applicable contract changes and record evidence for A79, A87, A89, A90 at this phase's layer. The primary completion phase remains the one in ACCEPTANCE; later integration/live tests are not falsely marked complete here.
