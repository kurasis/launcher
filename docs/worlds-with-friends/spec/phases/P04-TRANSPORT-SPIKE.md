# P04 — Transport and identity-aware TCP bridge spike

Milestone: **Core**. Initial status: **not_started**.
Dependencies: P03.

## Read first

Read [START-HERE](../START-HERE.md), the current [PROGRESS](../PROGRESS.md), and:

- [07-TUNNEL-PROTOCOL.md](../07-TUNNEL-PROTOCOL.md)
- [08-RELAY-OPERATIONS.md](../08-RELAY-OPERATIONS.md)
- [14-TESTING.md](../14-TESTING.md)

Additional required contract: [07-TUNNEL-PROTOCOL.md](../07-TUNNEL-PROTOCOL.md).

## Work

1. Compile a real iroh candidate and pin compatible agent/relay dependencies after testing.
2. Implement bounded admission framing and per-TCP-connection bidirectional stream bridging using test authorities explicitly marked as fixtures.
3. Prove direct and forced-relay paths, half-close behavior, backpressure, route observation and privacy policy controls.

### Revision 1.0.1 work

Prove there is no raw status service; isolate mixed privacy contexts and test global pre-auth limits. Use explicit test authorities for this spike without claiming P07 admission completion.

## Exit criteria

- [ ] Binary byte-for-byte forwarding, reconnect failure semantics and bounded memory tests pass.
- [ ] Relay-only packet captures show no direct probes or published candidates.
- [ ] A transport ADR documents exact versions, APIs, licenses, limitations and measured behavior.

- [ ] Revision 1.0.1 obligations applicable to this phase are implemented and evidenced; linked regression cases are: A53, A57. Later live-matrix gates remain explicitly pending where this phase only supplies the implementation.

## Evidence and handover

Store phase evidence under `docs/worlds-with-friends/evidence/P04/`. Include exact build/source identities, commands, environment, results, known limitations and whether each result used fixtures or live services. Add meaningful tests for the state transitions, trust boundaries and crash windows changed in this phase. Update generated contracts, migrations, user/operator instructions and [traceability](../TRACEABILITY.md) where affected.

Do not implement unrelated expansion features to bypass a blocker. Reuse qualified earlier facilities and test only the relevant regression surface plus mandatory gates. The exit criteria are all required; a blocked live integration is not a completed phase.

Follow the authorized branch/commit/push/PR/merge workflow in START-HERE. Update PROGRESS with implementation and evidence state before the final phase report. Report in Russian and keep repository artifacts in English.

## Revision 1.0.2 obligations

Test bounded incremental local handshake classification with exact byte preservation and per-listener/agent pending caps using protocol fixtures; full game qualification follows P08.

- [ ] Implement the applicable contract changes and record evidence for A86 at this phase's layer. The primary completion phase remains the one in ACCEPTANCE; later integration/live tests are not falsely marked complete here.
