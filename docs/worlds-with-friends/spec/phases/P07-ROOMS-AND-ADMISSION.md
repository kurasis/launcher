# P07 — Rooms, permissions, leases and verified admission

Milestone: **Core**. Initial status: **not_started**.
Dependencies: P02, P04, P05, P06.

## Read first

Read [START-HERE](../START-HERE.md), the current [PROGRESS](../PROGRESS.md), and:

- [03-OAUTH-AND-IDENTITY.md](../03-OAUTH-AND-IDENTITY.md)
- [04-ROOMS-AND-PERMISSIONS.md](../04-ROOMS-AND-PERMISSIONS.md)
- [07-TUNNEL-PROTOCOL.md](../07-TUNNEL-PROTOCOL.md)
- [09-API-AND-IPC.md](../09-API-AND-IPC.md)

Additional required contract: [18-AUTHORITY-AND-MAINTENANCE.md](../18-AUTHORITY-AND-MAINTENANCE.md).

## Work

1. Implement rooms, roles, invites, approvals, bans and owner transfer through the authoritative backend.
2. Implement host epochs/leases, single-use stream tickets, atomic consumption and renewable admission leases.
3. Wire the real game identity adapter and prove product-account to accepted-player binding.
4. Enforce privileged command permissions and host-device consent independently of the UI.

### Revision 1.0.1 work

Implement room-level start/claim, production pre-play socket binding, conservative leases, owner-transfer consent rules and backend maintenance authority. Purely post-join identity checks do not pass.

## Exit criteria

- [ ] Two real product accounts enter the selected real game session through the authorized bridge.
- [ ] Forged identity, replay, wrong endpoint, last-invite-use race and stale epoch are rejected.
- [ ] Removal, device revoke and authority outages close access within the stated lease limits; G04 live integration is proven.

- [ ] Revision 1.0.1 obligations applicable to this phase are implemented and evidenced; linked regression cases are: A54, A56, A59, A60, A71. Later live-matrix gates remain explicitly pending where this phase only supplies the implementation.

## Evidence and handover

Store phase evidence under `docs/worlds-with-friends/evidence/P07/`. Include exact build/source identities, commands, environment, results, known limitations and whether each result used fixtures or live services. Add meaningful tests for the state transitions, trust boundaries and crash windows changed in this phase. Update generated contracts, migrations, user/operator instructions and [traceability](../TRACEABILITY.md) where affected.

Do not implement unrelated expansion features to bypass a blocker. Reuse qualified earlier facilities and test only the relevant regression surface plus mandatory gates. The exit criteria are all required; a blocked live integration is not a completed phase.

Follow the authorized branch/commit/push/PR/merge workflow in START-HERE. Update PROGRESS with implementation and evidence state before the final phase report. Report in Russian and keep repository artifacts in English.

## Revision 1.0.2 obligations

Wire first-world prepare/commit/ack before the first playable room; this minimal transaction is a P07 dependency, while P09 adds general snapshots/restores. Implement cancel/claim fences, authorized replay reads, game-side release, unique world custody and stopped-room policy transition.

- [ ] Implement the applicable contract changes and record evidence for A78, A79, A80, A81, A87, A91 at this phase's layer. The primary completion phase remains the one in ACCEPTANCE; later integration/live tests are not falsely marked complete here.
