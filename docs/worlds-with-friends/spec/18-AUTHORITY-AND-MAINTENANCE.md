# 18 — Authority, maintenance and recovery transactions

## Different kinds of authority

| Record | Meaning | Lifetime / rule |
| --- | --- | --- |
| authorityEpoch | Backend recovery generation, kept outside rollback-prone database state | Changes after disaster recovery or explicit authority reset |
| room ownership/access revisions | Current account permissions | CAS updates; never inferred from a cached UI |
| placementRevision | Which device/world generation/pack pair is authoritative | Changes only through a committed typed transition |
| hostEpoch + sessionId | One gameplay process attempt | Host lease 30 s, renew 10 s; expired sessions do not revive |
| admission lease | One guest connection's right to continue | 30 s, renew 10 s, valid only with current host/placement authority |
| maintenance operation | Exclusive permission to perform a scoped data transition | 120 s lease, renew 20 s; expiry leaves a reconciliation barrier |

A worldGenerationId identifies a mutable working generation, not a digest of every live tick. Snapshot manifests identify immutable saved bytes with their own digests. Do not update backend placement on every world save or confuse a mutable world ID with a content hash.

## Maintenance permit

Kinds: initialize-world, import, snapshot, restore, export, delete-local-world, pack-transition, handoff. Core supports the first six; pack-transition and handoff activate only at their expansion milestones. Allocate a durable operation containing initiator account, assigned device(s), authorityEpoch, expected room/owner/access/placement revisions, worldId, kind, exact candidate/source IDs, permission scope, consent revision, state, request digest and timestamps.

Room API checks current role and device consent, then atomically installs an exclusive room maintenance barrier. If gameplay is active, the same host actor must perform its controlled stop; a separate maintenance process cannot steal the world lock. Reject competing start, restore, delete, pack change or handoff operations. Core serializes export through the same exclusive maintenance operation and a stopped room, including export of an existing snapshot. The snapshot remains pinned. Concurrent export from an immutable snapshot while gameplay continues is a future optimization requiring a separate read-permit contract; it is not implied by this barrier.

The actor retains its world reservation, renews maintenance authority while working and checks permission again before a destructive commit. Host gameplay leases continue only while the existing child is being stopped; after confirmed exit they become terminal. Maintenance authority continues independently for long compression, validation or transfer work.

Revocation/expiry stops further privileged side effects and candidate publication. Safely stop any validation child and preserve staging/old generations. Already committed backend decisions must be reconciled rather than guessed away. Room readiness remains blocked until a terminal result is established. Expiry never automatically deletes the barrier and lets another writer start.

## Recoverable world/pack commit

Use a write-ahead operation receipt and CAS protocol, not a fictional atomic transaction across the filesystem and backend:

1. **Prepare locally.** With the world stopped/reserved, create an isolated candidate, verify it and any disposable validation clone, flush payload and receipt. Record old/candidate world generation, packGeneration, candidate manifest digest and expected placementRevision. The active pointer is unchanged.
2. **Prepare backend.** Submit the receipt under the maintenance permit. Backend records the intended exact transition and current authorization; conflicting revisions fail. No room start is allowed while prepared state is unresolved.
3. **Commit backend.** Recheck permission, permit expiry, actor/device binding and expected placement. In one transaction CAS to the new placement/pack and record a durable commit receipt tied to operationId; room stays `reconciling`, not ready. A lost response is resolved by querying operationId. Do not send a new operation with new identity.
4. **Publish locally.** Verify the backend receipt matches this candidate, write world.json with new activeGenerationId, placementRevision and lastMutationId using the tested atomic replacement primitive, and flush required metadata. Preserve the previous generation.
5. **Acknowledge.** Report the matching local receipt. Backend clears the maintenance barrier only when custody and local publication agree. A new start operation may now allocate a new gameplay session if its actor still has permission.

For operations without a pointer change, use the result transaction below. Local deletion detaches the room's authoritative world reference first; no active room may point to deleted bytes.

## Crash and authorization decisions

| Failure window | Required action |
| --- | --- |
| Before backend prepare | Preserve old pointer; remove only owned uncommitted staging on explicit cleanup |
| Prepared, not committed | Query backend; abort only with confirmed uncommitted state and no living validation writer |
| Commit request result unknown | Keep barrier; query same operationId, never infer abort from timeout |
| Backend committed, local pointer old | Reconcile forward to the verified committed candidate; do not start old generation |
| Local pointer new, acknowledgment lost | Re-send idempotent acknowledgment; no second restore/import |
| Candidate missing/corrupt after backend commit | Mark repair-required, preserve all receipts and data, refuse readiness; use a separately authorized compensating operation |
| Actor loses role after commit | Permit only narrow reconciliation/acknowledgment by an otherwise valid assigned device; this grants no new start/export/restore rights |
| Device itself revoked | No privileged continuation using its revoked credential; keep barrier until a reauthorized operator/device reconciles custody |

Cancellation before backend commit aborts only after confirming uncommitted state. Cancellation after commit reports finishing/reconciling and cannot silently switch back. Compensating transitions are new revision-checked operations, not edits of past commit records.

## Agent crash and surviving Java

A restarted agent first acquires storage ownership and reconciles durable run reservations, native process start identity, executable and world path. An empty UI or released OS lock does not prove absence of a child. A PID alone is never enough for adoption or termination.

Old process stdin/stdout handles cannot be recreated merely by finding its PID. Adopt only if the implementation has a qualified authenticated process keeper or management channel and can recover control/health/identity. Otherwise mark orphaned, block new writes, and offer a verified controlled stop through an existing trusted management channel or explicit force-stop with unclean recovery warning. If the platform intentionally terminates children with a crashed agent, record the resulting unclean exit and verify data before restart. Seamless agent-crash survival is not a core guarantee.

An expired host lease cannot be resumed even if the process survived. Close channels, stop/reconcile the process and create a new session only after custody is clear. Read-only room browsing can continue while conflicting local mutation is blocked.

## Backend database disaster recovery

Before restoring database state, place service into maintenance and stop issuing/renewing product grants. Generate a new unpredictable authorityEpoch in a durable deployment authority outside the restored database, and rotate/revoke product grant keys as necessary. Include this epoch in host/admission/maintenance/relay permissions, commands and discovery responses. Every privileged endpoint compares it with current authority; never trust an epoch merely because an old token signature is valid.

Restore the database, but invalidate old sessions, pending starts, endpoint allow records and unresolved maintenance claims. Host counters remain useful within the new epoch; a restored smaller counter cannot equal previous global authority. Clients must obtain a fresh authenticated control snapshot before continuing.

A backup can lose membership/device revocations and ownership changes. Reconcile from a separately protected, non-rolled-back security/ownership journal. If that journal is unavailable, keep affected rooms frozen pending account/provider revalidation, device re-enrollment and explicit ownership/membership reconciliation. Do not reopen using stale permissions just because access tokens have not expired.

Local world receipts may be newer than the restored database. Preserve them; do not instruct a host to overwrite newer local data with an old backend pointer. Reconcile or explicitly fork history under operator review. No new authoritative gameplay session starts while that ambiguity remains.

## First-world publication

Room creation allocates roomId, worldId and the immutable baseline pack reference, but begins with `worldState=uninitialized`, no activeGenerationId and placementRevision zero. Such a room can be listed but cannot claim a gameplay session. Backend schemas explicitly permit this initial absence; it is not a missing-file recovery error.

Use initialize-world maintenance to create a new world. P06 provides the generator/isolated child primitive; P07 wires this minimum publication transaction before its first real room test. Imports use the same initial-publication branch in P09. Under current consent and maintenance authority, reserve resources, create an isolated candidate, generate it with the exact tuple and typed settings, stop the generator cleanly, verify its data and persist a receipt. Generation intentionally changes a new candidate; it is distinct from a restore validation clone. No public session or admission is exposed during generation.

Prepare/commit with expected absence of an active generation and placementRevision zero, then publish/ack the initial pointer through the same recoverable protocol. Preserve generated bytes across a lost commit response. A retry cannot replace a committed world with another random seed. Failed/cancelled initialization leaves the room uninitialized or reconciling according to its commit outcome. Only after acknowledgment may normal start allocate a gameplay session.

## Non-pointer result transaction

Snapshot/export use the same prepare, commit and ack endpoints with a discriminated result payload, not a fabricated new world placement. Prepare stores a verified snapshot/export digest, sizes, source generation, requested recipient/destination handle scope and exclusion policy. Commit checks current authority and records a result receipt without changing placementRevision. Stage content privately under the agent's custody before this decision; it is not a published successful backup or user-visible export yet. Ack records verified local publication. A response lost after commit is queried by operationId, never regenerated with a new operation.

Snapshot publication exposes only a complete immutable recovery object under managed permissions. Export has an additional disclosure boundary: recheck fresh export permission and device consent immediately before making bytes available through a user-selected file handle or later transfer service. Commit permission alone is not a permanent download capability. Revocation before disclosure leaves private staging unavailable; abort/reconcile metadata without revealing bytes. Revocation after bytes have been disclosed cannot recall an external copy. Record that boundary honestly. Core has no automatic export upload.

Local delete first commits detachment, then reconciles a managed trash move under custody. Physical purge is a separate retention action after pins and surviving processes are checked. Deleting a room while maintenance is pending closes access and records a deletion fence, but cannot erase its operation receipts or clear its custody barrier; committed decisions are reconciled, uncommitted ones cancelled. Reconciliation after room deletion never makes the room joinable again.

## Reconciliation claims for expired maintenance

Expose POST `/v1/maintenance-operations/{operationId}/recovery-claims`. It is not an ordinary renewal. The backend authenticates a currently authorized operator and enrolled device with current local consent, freezes the original decision, and CAS-allocates one new recoveryAttemptId/worker fence under the current authorityEpoch. A revoked credential or expired permit cannot claim recovery. The original operationId and committed outcome never change; a recovery attempt has its own 120-second lease, renews every 20 seconds through a typed recovery renewal, and is fenced on every acknowledgment or cleanup mutation.

Allowed actions depend on durable state: inspect scoped receipts; confirm abort of an uncommitted candidate after proving all writers stopped; reconcile the exact committed pointer/result; or report repair_required. Recovery cannot pick a different candidate, disclose an export, clear a barrier on timeout, issue gameplay grants or change ownership. A changed decision requires a separately authorized compensating operation after custody is established. If the assigned device was revoked, the local operator must re-enroll/reauthorize and explicitly claim the same managed storage; a random newly enrolled device does not prove filesystem custody.

The per-world actor must also fence stale local workers. Before committing, publishing, releasing locks or cleaning staging, compare the operation/recoveryAttemptId with its current durable worker fence. Cancel and await a previous worker before takeover; an unknown live process or unreachable custodian keeps the barrier blocked. Each worker owns distinct staging and cannot delete another attempt's candidate. The backend cannot establish local exclusivity by lease expiry alone.

When backend state and required receipts are unavailable, offer read-only recovery information and local safe stop. Automatic recovery must not guess an abort or select an old snapshot. Any manual fork is explicit and has new world/room identity. Core recovery covers the same local host; cross-device recovery remains subject to the handoff/custody gates.
