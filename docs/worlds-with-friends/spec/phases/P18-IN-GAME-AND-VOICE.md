# P18 — Optional integrated worlds and voice

Milestone: **Expansion**. Initial status: **not_started**.
Dependencies: P14.

## Read first

Read [START-HERE](../START-HERE.md), the current [PROGRESS](../PROGRESS.md), and:

- [16-ADVANCED-HOSTING.md](../16-ADVANCED-HOSTING.md)
- [07-TUNNEL-PROTOCOL.md](../07-TUNNEL-PROTOCOL.md)
- [13-SECURITY.md](../13-SECURITY.md)

Additional required contract: [07-TUNNEL-PROTOCOL.md](../07-TUNNEL-PROTOCOL.md).

## Work

1. Implement separately gated companion capabilities for selected loader/game tuples.
2. Add integrated-world publishing with explicit host-game lifetime and a clean export path.
3. Engineer voice as an independent opt-in transport with capture permission, mute/block and identity binding.

### Revision 1.0.1 work

Keep voice/integrated-world contexts separately authorized and privacy-scoped. They cannot reintroduce arbitrary UDP/LAN access or weaken gameplay pre-play binding.

## Exit criteria

- [ ] Each enabled capability has its own compatibility and privacy tests.
- [ ] Closing the integrated host game reports session termination honestly; no implied persistent hosting.
- [ ] Voice failure does not disrupt gameplay, expose arbitrary LAN services or bypass member blocking.

- [ ] Revision 1.0.1 obligations applicable to this phase are implemented and evidenced; linked regression cases are: A51, A52, A57. Later live-matrix gates remain explicitly pending where this phase only supplies the implementation.

## Evidence and handover

Store phase evidence under `docs/worlds-with-friends/evidence/P18/`. Include exact build/source identities, commands, environment, results, known limitations and whether each result used fixtures or live services. Add meaningful tests for the state transitions, trust boundaries and crash windows changed in this phase. Update generated contracts, migrations, user/operator instructions and [traceability](../TRACEABILITY.md) where affected.

Do not implement unrelated expansion features to bypass a blocker. Reuse qualified earlier facilities and test only the relevant regression surface plus mandatory gates. The exit criteria are all required; a blocked live integration is not a completed phase.

Follow the authorized branch/commit/push/PR/merge workflow in START-HERE. Update PROGRESS with implementation and evidence state before the final phase report. Report in Russian and keep repository artifacts in English.

## Revision 1.0.2 obligations

Qualify pre-play cancellation and game-side release semantics for integrated worlds; additional transports do not bypass current account/disclosure authorization.

- [ ] Implement the applicable contract changes and record evidence for A79, A80 at this phase's layer. The primary completion phase remains the one in ACCEPTANCE; later integration/live tests are not falsely marked complete here.
