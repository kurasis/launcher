# P12 — Core security, packaging and release qualification

Milestone: **Core**. Initial status: **not_started**.
Dependencies: P11.

## Read first

Read [START-HERE](../START-HERE.md), the current [PROGRESS](../PROGRESS.md), and:

- [13-SECURITY.md](../13-SECURITY.md)
- [14-TESTING.md](../14-TESTING.md)
- [15-DEPLOYMENT.md](../15-DEPLOYMENT.md)
- [ACCEPTANCE.md](../ACCEPTANCE.md)

Additional required contract: [AUDIT-REPORT-v1.0.2.md](../AUDIT-REPORT-v1.0.2.md).

## Work

1. Complete core acceptance, source/license provenance, package signing integration and update/drain behavior.
2. Run packaged binaries on all declared targets and a complete invitation-to-restored-world session.
3. Deliver operator runbooks, capacity report, deployment bundle, support matrix and an accurate release checklist.

### Revision 1.0.1 work

Require A01–A44, A53–A76 and A78–A91. Run non-rollback authority recovery and enforce the agreed platform/performance gates. A reduced preview cannot silently complete P12.

## Exit criteria

- [ ] All core acceptance cases pass on the declared matrix, with actual evidence and no unresolved release blocker.
- [ ] PROGRESS and traceability distinguish shipped support from expansion plans.
- [ ] Reviewable release artifacts are ready; public deployment/publication happens only under deployment authorization.

- [ ] Revision 1.0.1 obligations applicable to this phase are implemented and evidenced; linked regression cases are: A72, A73, A74, A76. Later live-matrix gates remain explicitly pending where this phase only supplies the implementation.

## Evidence and handover

Store phase evidence under `docs/worlds-with-friends/evidence/P12/`. Include exact build/source identities, commands, environment, results, known limitations and whether each result used fixtures or live services. Add meaningful tests for the state transitions, trust boundaries and crash windows changed in this phase. Update generated contracts, migrations, user/operator instructions and [traceability](../TRACEABILITY.md) where affected.

Do not implement unrelated expansion features to bypass a blocker. Reuse qualified earlier facilities and test only the relevant regression surface plus mandatory gates. The exit criteria are all required; a blocked live integration is not a completed phase.

Follow the authorized branch/commit/push/PR/merge workflow in START-HERE. Update PROGRESS with implementation and evidence state before the final phase report. Report in Russian and keep repository artifacts in English.

## Revision 1.0.2 obligations

Require all 82 current core cases, including direct-file log privacy, desktop/backend fetch restrictions and recovery-retention evidence. A written requirement or fixture-only success does not satisfy live gates.

- [ ] Implement the applicable contract changes and record evidence for A78, A79, A80, A81, A82, A83, A84, A85, A86, A87, A88, A89, A90, A91 at this phase's layer. The primary completion phase remains the one in ACCEPTANCE; later integration/live tests are not falsely marked complete here.
