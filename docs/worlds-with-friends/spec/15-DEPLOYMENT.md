# 15 — Deployment, updates and observability

## Environments

Provide local development, isolated staging and production configurations. Development may use a clearly marked test account provider; it cannot satisfy live identity gates. Staging uses separate client registration, signing keys, relay allow records and data. No shared secrets between environments.

Pin Rust/Node tooling, package manager, dependencies, relay build and container image digests in repository-owned files. Build desktop agent binaries for each target triple and verify their digest/signature in the packaging pipeline. Sign/notarize supported packages through the project's release mechanism. Do not invent signing credentials.

## Configuration inventory

| Variable/config group | Owner | Secret? |
| --- | --- | --- |
| Account issuer, public client ID, scopes, Room API origin | Deployment | No; integrity-sensitive |
| Token validation/introspection credentials if required | Backend secret store | Yes |
| Room grant signing keys and key IDs | Backend secret store | Private portion yes |
| Database URL and migration role | Backend secret store | Yes |
| Relay URLs, trusted endpoint discovery policy | Deployment | No; integrity-sensitive |
| Relay access service credential/policy channel | Relay secret store | Yes |
| Device private keys and refresh credentials | OS vault | Yes |
| Metrics listen addresses and retention | Operations | Restricted |
| Feature gates and support tuple catalog | Signed/reviewed release policy | Integrity-sensitive |

Never place production values in example files. Startup validates complete configuration and refuses a public production listener with test keys or allow-all relay access.

## Database and local upgrades

Use additive backend schema changes first, maintain a documented compatibility window, then remove obsolete fields after supported clients migrate. Back up the backend database and prove restore into staging. Database restore must account for expired leases, revoked keys and idempotency records; restored old rows cannot resurrect active host authority.

Local upgrades drain or postpone during sessions. Preserve old executable and storage checkpoint where rollback is safe; a database format downgrade may be unsupported and must be detected before writing. Test previous supported agent connecting to the new backend and vice versa within the declared protocol range.

## Observability

Metrics: authorization failures by safe category, room/session state transitions, host lease failures, admitted connections, direct/relay proportions, time to connect, stream errors, node bytes, queue pressure, Java exits, snapshot verification failures, disk pressure and orphan recovery. Correlate with opaque request/session IDs in restricted logs; avoid unbounded per-user labels in metrics.

Expose health separately: liveness means process responsive; readiness means configuration, required dependencies and capacity allow work. A live relay with denied access policy or exhausted capacity is not ready. Probe the actual transport path, not just an HTTP landing page.

## Required runbooks

Provide procedures for account-provider outage, Room API outage, database restore, relay saturation, key compromise, certificate failure, host orphan process, corrupted snapshot, bad pack generation, failed desktop update and data export/deletion. Each names observable symptoms, allowed actions, expected user impact and recovery verification.

## Release gate

P12 delivers a reviewable deployment bundle and evidence. Build reproducible packages, pass core acceptance, record source provenance/licenses, verify retention and access policy, and run a packaged two-device session. Public production deployment and paid resources require the owner's deployment authorization; automatic Git push/merge alone does not supply it.

Keep the core service available without cloud-hosting or voice dependencies. An optional subsystem outage must not crash a local server agent. Core control-service outages still follow the bounded authorization-expiry policy.

## Database recovery generation

The backend recovery-generation procedure in [18](18-AUTHORITY-AND-MAINTENANCE.md) is mandatory. Restoring a backup can roll back revocations, membership and host counters; fresh expiring leases alone do not repair that history. Keep the recovery authority outside the restored database, invalidate prior grants/endpoints, reconcile audit authority and require reauthorization before reopening service.

## Recovery retention is distinct from log retention

The 30-day ordinary security-event default is not permission to discard the non-rollback security/ownership journal, unresolved operation receipts, world custody barriers or deduplication tombstones needed for recovery. Define a deployment recovery horizon that covers every restorable database backup plus the maximum accepted delayed-command/retry window. Retain the minimal authoritative checkpoint and subsequent changes for that horizon; safely compact it only after corresponding older backups and replay paths are retired. Never restore a database older than the available authority checkpoint and open rooms without explicit reconciliation.

Room/account deletion cancels new disclosure and grants promptly, but physical removal of recovery records is deferred or compacted into minimal nonsecret tombstones as required by the documented data policy. Do not preserve expired invite secrets merely to preserve an operation outcome. A purge cannot delete receipts for unresolved maintenance or a living/uncertain process. Test deletion during a prepared and committed restore, a retry after the generic result cache expires, and disaster recovery after ordinary audit-log rotation.
