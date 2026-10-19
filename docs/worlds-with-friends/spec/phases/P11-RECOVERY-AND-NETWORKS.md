# P11 — Fault recovery and real network qualification

Milestone: **Core**. Initial status: **not_started**.
Dependencies: P05, P07, P09, P10.

## Read first

Read [START-HERE](../START-HERE.md), the current [PROGRESS](../PROGRESS.md), and:

- [07-TUNNEL-PROTOCOL.md](../07-TUNNEL-PROTOCOL.md)
- [08-RELAY-OPERATIONS.md](../08-RELAY-OPERATIONS.md)
- [14-TESTING.md](../14-TESTING.md)

Additional required contract: [18-AUTHORITY-AND-MAINTENANCE.md](../18-AUTHORITY-AND-MAINTENANCE.md).

## Work

1. Implement and test sleep/resume, agent/Java crash recovery, stale receipts and controlled relay reconnection.
2. Exercise residential NAT, CGNAT, double NAT, IPv6, blocked UDP, packet loss and mixed OS pairs.
3. Run real session and transport soaks; capture route, latency, revocation and memory evidence.

### Revision 1.0.1 work

Inject delayed lease responses, mixed privacy sessions, agent crash with surviving Java and each local/backend commit failure window. Report observed limitations.

## Exit criteria

- [ ] Network matrix records direct/relay outcomes and game rejoin requirements without claiming universal connectivity.
- [ ] Two-relay failure tests and privacy captures pass for the release configuration.
- [ ] No second authoritative world writer or unbounded memory growth appears in the defined failure tests.

- [ ] Revision 1.0.1 obligations applicable to this phase are implemented and evidenced; linked regression cases are: A56, A57, A61, A75. Later live-matrix gates remain explicitly pending where this phase only supplies the implementation.

## Evidence and handover

Store phase evidence under `docs/worlds-with-friends/evidence/P11/`. Include exact build/source identities, commands, environment, results, known limitations and whether each result used fixtures or live services. Add meaningful tests for the state transitions, trust boundaries and crash windows changed in this phase. Update generated contracts, migrations, user/operator instructions and [traceability](../TRACEABILITY.md) where affected.

Do not implement unrelated expansion features to bypass a blocker. Reuse qualified earlier facilities and test only the relevant regression surface plus mandatory gates. The exit criteria are all required; a blocked live integration is not a completed phase.

Follow the authorized branch/commit/push/PR/merge workflow in START-HERE. Update PROGRESS with implementation and evidence state before the final phase report. Report in Russian and keep repository artifacts in English.

## Revision 1.0.2 obligations

Stress lease scheduling with real disk/CPU/control-load saturation and clock/resume faults. Inject old-worker wakeup and socket teardown races as well as normal network outages.

- [ ] Implement the applicable contract changes and record evidence for A80, A86, A88, A89 at this phase's layer. The primary completion phase remains the one in ACCEPTANCE; later integration/live tests are not falsely marked complete here.
