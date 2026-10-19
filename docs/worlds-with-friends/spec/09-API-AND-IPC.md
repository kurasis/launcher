# 09 — API, events and local IPC contracts

## Contract ownership

This document specifies a proposed `/v1` product API. If an existing service already supplies rooms/OAuth, implement an explicit mapping and contract tests instead of deploying a competing source of truth. Freeze OpenAPI 3.1 and JSON schemas in P01/P02, and keep generated client DTOs in CI. URLs here are relative API routes, not discovered production endpoints.

All protected operations require a validated resource access token plus resource-level authorization. IDs are opaque UUIDs. Times are UTC RFC3339, durations integer milliseconds, byte counts integer bytes and hashes lowercase algorithm-tagged hexadecimal. JSON fields use camelCase. Database counters/revisions transported to TypeScript are bounded safe integers or decimal strings, consistently declared by schema.

Device-scoped operations additionally require the registered-key proof contract in [03](03-OAUTH-AND-IDENTITY.md). Generate this requirement into the API security schemes and test it; an account bearer token plus a claimed deviceId alone cannot renew a host lease or acknowledge another device's command.

## Core endpoints

| Method and route | Effect | Guard |
| --- | --- | --- |
| GET `/v1/me` | Canonical own account and capabilities | Valid account token |
| POST `/v1/device-challenges` | One-use key-registration challenge | Account + enrollment limit |
| POST `/v1/devices` | Register stable device control key with challenge proof | Account + proof + idempotency |
| DELETE `/v1/devices/{id}` | Revoke device and affected access | Device owner |
| GET/POST `/v1/rooms` | List memberships / create private room | Account + room quota |
| GET/PATCH `/v1/rooms/{id}` | Snapshot / permitted metadata changes | Membership / role + revision |
| DELETE `/v1/rooms/{id}` | Tombstone room and revoke admissions | Owner + revision |
| POST `/v1/rooms/{id}/invites` | Create scoped member invite | Owner or manager |
| POST `/v1/invites/preview` | Minimal invite preview; token in body | Account + rate limit |
| POST `/v1/invites/redeem` | Request/activate membership | Account + invite checks |
| DELETE `/v1/rooms/{id}/invites/{inviteId}` | Revoke invite | Owner or manager |
| GET `/v1/rooms/{id}/members` | List visible membership/roles | Membership |
| PATCH/DELETE `/v1/rooms/{id}/members/{accountId}` | Role, approval or removal | Permission matrix + revision |
| POST `/v1/rooms/{id}/ownership-transfers` | Propose owner transfer | Owner + recipient |
| POST `/v1/ownership-transfers/{id}/accept` | Accept transfer transactionally | Intended recipient |
| POST `/v1/rooms/{id}/start-operations` | Request start while no active session exists and world is initialized; return operationId | Role + room/owner/placement revisions + assigned-host consent |
| POST `/v1/start-operations/{operationId}/claim` | Assigned host claims exactly one session/epoch | Device proof + current consent + same revisions |
| GET `/v1/operations/{operationId}` | Read durable operation result/expiry | Authorized initiator or assigned device |
| POST `/v1/devices/{id}/endpoint-bindings` | Bind a scoped transport key to the device | Account + device and endpoint proofs |
| DELETE `/v1/devices/{id}/endpoint-bindings/{bindingId}` | Revoke one endpoint context | Device owner or matching device proof |
| POST `/v1/sessions/{id}/heartbeat` | Renew host lease and report observation | Matching device/session/epoch |
| POST `/v1/sessions/{id}/commands` | Enqueue stop for an existing exact session; never start | Role + device consent + authority/session/host epoch |
| POST `/v1/sessions/{id}/finalize` | Report stopped/failed and process custody receipt | Matching host proof + lifecycle state checks |
| GET `/v1/sessions/{id}/status` | Bounded sanitized status DTO, never raw TCP proxying | Current membership + response age limit |
| POST `/v1/rooms/{id}/maintenance-operations` | Allocate typed world-operation authority | Role + device consent + revisions + no conflicting operation |
| POST `/v1/maintenance-operations/{operationId}/renew` | Renew operation lease | Assigned device proof + current actor permission |
| POST `/v1/maintenance-operations/{operationId}/prepare` | Prepare discriminated pointer transition or non-pointer result receipt | Matching operation + expected placement revision |
| POST `/v1/maintenance-operations/{operationId}/commit` | Commit prepared placement or non-pointer result under current authority | Valid prepared operation + current permissions |
| POST `/v1/maintenance-operations/{operationId}/ack` | Confirm local publication or terminal non-pointer operation | Assigned host + exact committed receipt |
| POST `/v1/maintenance-operations/{operationId}/abort` | Cancel before commit or mark reconciliation required | Authorized actor/device + current state |
| GET `/v1/devices/{id}/commands` | Poll bounded durable command inbox | Matching device identity |
| POST `/v1/commands/{id}/ack` | Durable execution state/result | Matching executing device |
| POST `/v1/sessions/{id}/admissions` | Issue a short-lived single-use ticket | Current member + registered device |
| POST `/v1/admissions/consume` | Atomically consume and allocate lease/slot | Matching host and verified peer binding |
| POST `/v1/sessions/{id}/admission-renewals` | Renew/revoke connection leases in a batch | Matching host epoch |
| DELETE `/v1/sessions/{id}/admissions/{connectionId}` | Release/close admission | Owning guest or authorized host |
| GET `/v1/rooms/{id}/events` | Authorized resumable event stream | Current membership |
| GET `/v1/rooms/{id}/packs/{generation}` | Immutable authorized pack manifest | Membership |

Core room creation uses a curated supportTupleId and typed world inputs to produce the baseline immutable manifest; guests retrieve it using the pack endpoint. Arbitrary modpack publication starts in P13. Snapshot bytes, local inventory, extraction and export stay in typed local IPC, but current authority for world operations is obtained through the maintenance API. Core operation execution is restricted to the local assigned host; remote-device execution remains gated to P15. Device consent can be revoked locally at any time and cannot be granted by a remote room owner. No API accepts a raw terminal command.

Start operation defaults: 120-second claim window after preflight; bind expected roomRevision, ownerRevision, placementRevision, assigned device and expected absence of an active session/maintenance operation. Claim plus session allocation is one transaction and is idempotent for the operation. A stale start cannot restart a room after the owner has stopped or changed it; enforce the executionRevision/cancellation fence below rather than relying only on unrelated metadata revisions. Long downloads happen before claim; once claimed, the new session follows its own bounded startup and host-lease renewals.

## Representative request

POST `/v1/sessions/{sessionId}/admissions`:

```json
{
  "deviceId": "00000000-0000-4000-8000-000000000001",
  "endpointId": "registered-transport-key",
  "expectedHostEpoch": "42",
  "packGeneration": "sha256:example-digest",
  "service": "gameplay",
  "protocolVersion": 1
}
```

The server derives accountId from the token and verifies the device/endpoint relationship; it does not trust a supplied accountId. Example identifiers are documentation placeholders and never valid test evidence. Response contains ticket, expiresAt, expected host endpoint, session epoch, protocol version and route policy. Discovery fields honor relay-only restrictions.

## Idempotency and optimistic concurrency

Every non-idempotent mutation requires `Idempotency-Key`, scoped to principal and route, with request-body hash. Persist operation and nonsecret result metadata atomically with the business change. Retain the idempotency record for 24 hours initially; reusing a key with another body returns conflict. Secret response material is not stored in plaintext inside a generic request/response journal. For invite creation, retain any exact retry response encrypted for at most 10 minutes and no longer than invite expiry; after that return INVITE_RESULT_EXPIRED with the original invite ID rather than silently creating a second invite. Admission-ticket retry payloads expire no later than the ticket. A known completed operation never becomes permission to issue fresh secrets on an old retry. Ticket-consumption replay state survives at least ticket expiry plus maximum validation skew and is separately keyed by ticket/connection IDs.

PATCH and destructive changes require an expected revision (`If-Match` or a consistently generated equivalent). Compare-and-swap and uniqueness constraints protect membership, last invite use, host lease and owner transfers. Use transactional outbox events after commit. Do not promise exactly-once network delivery.

## Errors

Envelope fields: code, messageKey, safeDetails, retryable, retryAfterMs, correlationId and optional operationId. Never include credentials, raw launch arguments or unrestricted paths. Standard HTTP mapping: 400 validation, 401 invalid authentication, 403 forbidden, 404 missing/not-visible, 409 revision/state conflict, 410 expired invite, 429 quota, 503 unavailable dependency.

Required codes include AUTH_REQUIRED, PROVIDER_UNAVAILABLE, DEVICE_REVOKED, ROOM_FORBIDDEN, INVITE_EXPIRED, INVITE_EXHAUSTED, APPROVAL_REQUIRED, REVISION_CONFLICT, HOST_OFFLINE, HOST_LEASE_LOST, SESSION_NOT_READY, ROOM_FULL, PACK_MISMATCH, IDENTITY_BINDING_UNAVAILABLE, IDENTITY_MISMATCH, TICKET_REPLAY, TRANSPORT_UNAVAILABLE, RELAY_CAPACITY, REJOIN_REQUIRED, DISK_INSUFFICIENT, WORLD_BUSY, WORLD_UNCLEAN, SERVER_START_FAILED, STOP_TIMEOUT, SNAPSHOT_INVALID, IPC_VERSION_UNSUPPORTED and OPERATION_INTERRUPTED. Also require ALREADY_CONNECTED, SERVICE_NOT_ALLOWED, AUTHORITY_CHANGED, MAINTENANCE_BUSY, OPERATION_EXPIRED, IDENTITY_TIMEOUT, STALE_LEASE_RESPONSE and INVITE_RESULT_EXPIRED.

## Events

Envelope: schemaVersion, streamEpoch, eventId, resourceId, resourceRevision, kind, emittedAt and redacted payload. Events include roomChanged, membershipChanged, inviteRevoked, hostStateChanged, sessionStateChanged, admissionRevoked, jobProgress and operationCompleted.

Client obtains a snapshot plus cursor, then consumes later events. Duplicates are ignored by ID/revision. A gap, stale cursor, restart epoch or permission change causes a new snapshot; unauthorized subscriptions close. Recheck authorization at least every 10 seconds and on revocation notifications. Durable state is authoritative, not delivery of a final UI event. Event data never includes world contents or tokens.

## Local IPC

Prefer same-user Unix sockets with restrictive permissions on Unix systems and named pipes with explicit current-user ACLs on Windows. Authenticate the UI peer and negotiate protocol versions. If a loopback HTTP fallback is justified, bind loopback, require a per-install credential held outside the webview, validate origins, disable browser ambient authority and test DNS rebinding/CSRF paths. Loopback is not sufficient authentication.

All commands use requestId and expectedRevision, return operationId, and support query-by-operation for reconnection. A GUI reload reattaches to an operation; it does not start it again. Define typed `rooms.list`, `rooms.join`, `server.prepare/start/stop`, `world.import/snapshot/restore/export`, `job.cancel`, `agent.snapshot`, `diagnostics.export` and `agent.shutdown` operations. Cap requests at 256 KiB; media/file transfers use dedicated bounded handles, not giant JSON.

Commands are durable desired operations, not blind scripts. On restart, an agent reconciles a command's intended result with process receipts before retrying. Session commands expire and carry target host epoch; a delayed stop must never stop a newer session.

## API completeness and DTO invariants

P02/P07 generate explicit schemas and permission tests for every listed operation, including device-scoped proof, expected revisions, idempotency, request caps and terminal results. Core membership PATCH supports only documented role/approval/ban-clear transitions; ownership and protected world references have separate APIs.

Use `packGeneration` as the platform-neutral `sha256:<digest>` identity throughout API/tickets. Installation generation IDs and platform-specific lock hashes are different fields. `authorityEpoch` is a random recovery-generation identifier; `hostEpoch`, `placementRevision`, `ownerRevision` and other counters are decimal strings on the wire. No client may reset them by supplying a smaller number.

Lease responses contain request nonce/sequence, serverIssuedAt, expiresAt, authorityEpoch, the scoped host/operation/admission identifiers and current access revision. Original expiry is preserved on retries. A replayed completed response cannot revive expired state. Events carry authorityEpoch in addition to streamEpoch; any change invalidates cursors and requires re-authenticated snapshots.

## Start cancellation and execution fences

Expose POST `/v1/start-operations/{operationId}/cancel` with current actor permission, expected operation revision and idempotency. A start operation has `pending`, `claimed`, `cancel_requested`, `cancelled`, `completed` or `failed` state; expiry before claim is terminal. Persist cancel intent before acknowledging it. Claim and cancel serialize on the same operation row. Cancel winning before claim prevents session allocation. Claim winning first returns its exact sessionId and sets cancel_requested; it never reports that no process can exist.

The assigned actor checks cancellation before spawn and reconciles any spawn already in flight. A cancellation that races spawn follows the controlled stop path and remains pending until actual exit/custody is known. A response timeout is resolved by querying the same operation. Local job.cancel maps to this operation, including during preflight; cancellation before a remote operation exists is durable locally and prohibits its later creation. No generic cancel signal can target an unrelated/newer session.

An explicit room Stop also cancels pending starts transactionally and advances a room executionRevision. Start requests/claims carry that revision. Implement POST `/v1/rooms/{id}/stop-operations` with the current start/stop role, assigned-device consent and expected executionRevision for this room-level intent; it targets the current session if one exists and cancels earlier pending starts in the same transaction. Ordinary repeated stop is safe. Physical local stop remains available when backend authorization is unavailable; synchronize the stop fence on reconnect before accepting delayed commands. Local resource shutdown never depends on acquiring a maintenance permit.

## Authorization of retries and result reads

Authenticate and validate current device/account authority before looking up or returning an idempotency result. Recheck resource visibility and permission to disclose the result, including encrypted secret responses, on every replay and operation query. A previous successful request is not a capability to read a private room after removal. Keep internal execution deduplication separate from response disclosure: refusing disclosure must never execute the operation again. Only the narrowly scoped assigned-device reconciliation receipt in [18](18-AUTHORITY-AND-MAINTENANCE.md) may remain readable after loss of an initiating role; it contains no invite or private export bytes.

Scope durable deduplication to principal, HTTP method, normalized concrete resource route and request digest. The 24-hour generic result cache is not the lifetime of a destructive operation identity. Persist operation identities/outcomes or compact tombstones through the lifecycle/recovery horizon; never automatically resubmit an old unknown operation with a new key. After retention, an unresolved retry returns RESULT_UNAVAILABLE and requires reconciliation. P02 defines this mapping explicitly for any existing backend instead of treating key expiry as proof that the original side effect never occurred.

## Recovery API completion

Implement POST `/v1/maintenance-operations/{operationId}/recovery-claims` and POST `/v1/recovery-claims/{recoveryAttemptId}/renew`, plus a typed POST `/v1/recovery-claims/{recoveryAttemptId}/actions` accepting only `ack_committed`, `abort_uncommitted` or `report_repair_required`. They carry current authorityEpoch, expected operation/worker revisions, device proof and validated custody receipts. Return CONFLICTING_RECOVERY or CUSTODY_UNCONFIRMED rather than clearing a barrier heuristically. The canonical permission/action matrix is [18](18-AUTHORITY-AND-MAINTENANCE.md); no shell/file-operation parameters are accepted.

Add POST `/v1/rooms/{id}/route-policy-transitions` with the stopped-room guards in [04](04-ROOMS-AND-PERMISSIONS.md). Include executionRevision and routePolicyRevision as decimal strings in generated room/start/endpoint DTOs where applicable, and routePolicyRevision in tickets/discovery. API schemas also include the start-cancel and room-stop routes above. New required errors are RESULT_UNAVAILABLE, CONNECTION_CLOSING, RESTORE_TUPLE_UNAVAILABLE, CONFLICTING_RECOVERY and CUSTODY_UNCONFIRMED.
