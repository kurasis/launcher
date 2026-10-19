# P17 — Optional budgeted cloud hosting

Milestone: **Expansion**. Initial status: **not_started**.
Dependencies: P16.

## Read first

Read [START-HERE](../START-HERE.md), the current [PROGRESS](../PROGRESS.md), and:

- [16-ADVANCED-HOSTING.md](../16-ADVANCED-HOSTING.md)
- [15-DEPLOYMENT.md](../15-DEPLOYMENT.md)
- [13-SECURITY.md](../13-SECURITY.md)

Additional required contract: [16-ADVANCED-HOSTING.md](../16-ADVANCED-HOSTING.md).

## Work

1. Integrate a selected provider with explicit purchase consent, region, budgets and resource isolation.
2. Implement persistent snapshot custody, quota/metering, export/deletion and idle stop.
3. Use the same handoff protocol for home-to-cloud and cloud-to-home transitions.

### Revision 1.0.1 work

Reuse the exact handoff authority protocol and validate a provider-specific spending reservation/overrun model. Do not promise exact external billing caps from delayed metrics.

## Exit criteria

- [ ] Budget and authorization tests prevent unapproved provisioning and unlimited spending.
- [ ] Provider outage and restore exercises demonstrate documented recovery objectives.
- [ ] A room can return to a home host with verified data and all cloud resources accounted for.

- [ ] Revision 1.0.1 obligations applicable to this phase are implemented and evidenced; linked regression cases are: A50, A77. Later live-matrix gates remain explicitly pending where this phase only supplies the implementation.

## Evidence and handover

Store phase evidence under `docs/worlds-with-friends/evidence/P17/`. Include exact build/source identities, commands, environment, results, known limitations and whether each result used fixtures or live services. Add meaningful tests for the state transitions, trust boundaries and crash windows changed in this phase. Update generated contracts, migrations, user/operator instructions and [traceability](../TRACEABILITY.md) where affected.

Do not implement unrelated expansion features to bypass a blocker. Reuse qualified earlier facilities and test only the relevant regression surface plus mandatory gates. The exit criteria are all required; a blocked live integration is not a completed phase.

Follow the authorized branch/commit/push/PR/merge workflow in START-HERE. Update PROGRESS with implementation and evidence state before the final phase report. Report in Russian and keep repository artifacts in English.

## Revision 1.0.2 obligations

Apply download, disclosure and recovery-retention policies to cloud hosts and external transfers without weakening host-custody gates.

- [ ] Implement the applicable contract changes and record evidence for A82, A85, A90 at this phase's layer. The primary completion phase remains the one in ACCEPTANCE; later integration/live tests are not falsely marked complete here.
