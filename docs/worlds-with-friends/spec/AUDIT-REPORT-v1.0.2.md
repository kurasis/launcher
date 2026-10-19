# Second audit report — revision 1.0.2

Date: 2026-10-19 (Asia/Dubai). Input: the delivered v1.0.1 Markdown specification. Output: a separate corrected copy. Original archives are preserved. This is a design/requirements review; no implementation repository, game run, penetration test or live deployment is included.

## Scope and interpretation

The review focuses on transitions between asynchronous steps, authority loss, retry behavior, filesystem publication and adapters. Findings describe real gaps or conflicting requirements in the written contract, not observed bugs in deployed code. F01–F25 remain in the [previous report](AUDIT-REPORT-v1.0.1.md); F26–F39 are additional. Severity estimates the consequence of implementing the gap literally.

| Finding | Severity | Problem | Normative correction | Regression |
| --- | --- | --- | --- | --- |
| F26 | High | Pending start had no cancellation route | [09-API-AND-IPC.md](09-API-AND-IPC.md) | A78 |
| F27 | High | Cached retry results could bypass current authorization | [09-API-AND-IPC.md](09-API-AND-IPC.md) | A79 |
| F28 | High | Expired lease or closed TCP socket could free a player slot too early | [07-TUNNEL-PROTOCOL.md](07-TUNNEL-PROTOCOL.md) | A80 |
| F29 | High | The first world had no initial publication transaction | [18-AUTHORITY-AND-MAINTENANCE.md](18-AUTHORITY-AND-MAINTENANCE.md) | A81 |
| F30 | High | Non-pointer maintenance and export disclosure were underspecified | [18-AUTHORITY-AND-MAINTENANCE.md](18-AUTHORITY-AND-MAINTENANCE.md) | A82 |
| F31 | High | Restore did not explicitly bind exact pack and persistent player mapping | [11-WORLDS-AND-BACKUPS.md](11-WORLDS-AND-BACKUPS.md) | A83 |
| F32 | High | Filtering pipes did not control game-created log and dump files | [13-SECURITY.md](13-SECURITY.md) | A84 |
| F33 | High | Content-fetch restrictions did not explicitly cover desktop requests and credentials | [13-SECURITY.md](13-SECURITY.md) | A85 |
| F34 | Medium | Local handshake classification could lose bytes or allocate unbounded work | [07-TUNNEL-PROTOCOL.md](07-TUNNEL-PROTOCOL.md) | A86 |
| F35 | High | One-session-per-room did not prohibit two rooms claiming one world | [04-ROOMS-AND-PERMISSIONS.md](04-ROOMS-AND-PERMISSIONS.md) | A87 |
| F36 | High | Expired maintenance barriers lacked a fenced recovery claim | [18-AUTHORITY-AND-MAINTENANCE.md](18-AUTHORITY-AND-MAINTENANCE.md) | A88 |
| F37 | High | Control deadlines could be starved or reused across a clock epoch | [03-OAUTH-AND-IDENTITY.md](03-OAUTH-AND-IDENTITY.md) | A89 |
| F38 | High | Ordinary audit retention could erase disaster-recovery authority | [15-DEPLOYMENT.md](15-DEPLOYMENT.md) | A90 |
| F39 | Medium | Route policy was protected against PATCH but had no typed transition | [04-ROOMS-AND-PERMISSIONS.md](04-ROOMS-AND-PERMISSIONS.md) | A91 |

## Evidence and resolution

### F26 — Pending start had no cancellation route

In v1.0.1: The start route allocated a session only on claim, while stop commands required an existing session. Cancelling between request and claim had no defined durable effect.

Correction and required evidence: Race cancel/room Stop with pending claim, delayed download completion and process spawn. Cancel-first prevents allocation; claim-first stops the exact child and waits for confirmed exit. A delayed old request cannot restart the room. See [09-API-AND-IPC.md](09-API-AND-IPC.md); primary acceptance is P07, A78.

### F27 — Cached retry results could bypass current authorization

In v1.0.1: Secret response encryption/retention was defined, but current authorization on replay was not; the 24-hour cache also left old operation retries ambiguous.

Correction and required evidence: Create an invite/export operation, revoke the caller, then replay its key and query its result. No private result/secret is disclosed and no duplicate side effect runs. Repeat after generic cache expiry using the same durable operation ID. See [09-API-AND-IPC.md](09-API-AND-IPC.md); primary acceptance is P07, A79.

### F28 — Expired lease or closed TCP socket could free a player slot too early

In v1.0.1: Replacement permitted waiting for lease expiry as an alternative to proven closure, without proving player removal/save or cancelling a pending pre-play login.

Correction and required evidence: Delay game-side removal/save and pending-login cancellation after transport close or lease expiry. Rejoin/replacement remains blocked until matching adapter release or confirmed server exit. A stale acknowledgment cannot release a replacement. See [07-TUNNEL-PROTOCOL.md](07-TUNNEL-PROTOCOL.md); primary acceptance is P07, A80.

### F29 — The first world had no initial publication transaction

In v1.0.1: Normal start required a validated active world receipt, but creation did not define how the first world obtained backend/local authority before that receipt existed.

Correction and required evidence: Create a new room, crash during generation and lose initial commit/ack responses. Uninitialized rooms reject gameplay; exactly one verified generated world is committed and retries preserve its seed/data. P07 can pass without waiting for P09 import. See [18-AUTHORITY-AND-MAINTENANCE.md](18-AUTHORITY-AND-MAINTENANCE.md); primary acceptance is P07, A81.

### F30 — Non-pointer maintenance and export disclosure were underspecified

In v1.0.1: Snapshot/export reused pointer-oriented endpoints without a result schema, and live immutable export conflicted with the exclusive barrier. Export authorization had no final disclosure boundary.

Correction and required evidence: Prepare snapshot/export, revoke permission before disclosure, and lose result commit/ack responses. Private staging stays private; one result is reconciled without a placement bump. Delete the room during maintenance and verify receipts/barriers survive and joins stay closed. See [18-AUTHORITY-AND-MAINTENANCE.md](18-AUTHORITY-AND-MAINTENANCE.md); primary acceptance is P09, A82.

### F31 — Restore did not explicitly bind exact pack and persistent player mapping

In v1.0.1: Required runtime availability was mentioned, but substituting the room current pack or a changed account/profile mapping was not explicitly prohibited; validation and failure wording remained ambiguous.

Correction and required evidence: Restore with an unavailable/different required tuple, renamed account, reused nickname and changed identity namespace. Block unsupported mappings/build substitution before destructive work. After a committed-restore crash reconcile forward; never present the old pointer as current. See [11-WORLDS-AND-BACKUPS.md](11-WORLDS-AND-BACKUPS.md); primary acceptance is P09, A83.

### F32 — Filtering pipes did not control game-created log and dump files

In v1.0.1: The previous logging correction covered agent persistence of stdout/stderr but not direct file appenders, Java crash reports or heap dumps copied into diagnostics/backups.

Correction and required evidence: Emit synthetic secrets/chat, trigger a controlled crash and inspect actual run files, snapshots and diagnostics. Qualified default sinks follow policy; raw debug/dumps require explicit opt-in and never upload automatically. See [13-SECURITY.md](13-SECURITY.md); primary acceptance is P12, A84.

### F33 — Content-fetch restrictions did not explicitly cover desktop requests and credentials

In v1.0.1: Redirect-to-private-address checks named API-side fetches; native guest/host downloaders and cross-origin credential forwarding were not fully specified.

Correction and required evidence: On desktop and backend, try private-address redirects, DNS rebinding, mapped IPv6, credential-bearing URLs and cross-origin redirects. No restricted destination is contacted and no control credential reaches content origins. Approved sources still install normally. See [13-SECURITY.md](13-SECURITY.md); primary acceptance is P12, A85.

### F34 — Local handshake classification could lose bytes or allocate unbounded work

In v1.0.1: The local status parser had a size/deadline but no exact consumed-byte handoff or local socket cap; it also lacked explicit run/session binding and adapter-registration ordering.

Correction and required evidence: Fragment/coalesce handshake and login bytes, race trusted binding registration, flood local sockets and end the client run. Gameplay sees exactly the original ordered bytes, no early pre-play entry, bounded pending work and no stale listener retargeting. See [07-TUNNEL-PROTOCOL.md](07-TUNNEL-PROTOCOL.md); primary acceptance is P08, A86.

### F35 — One-session-per-room did not prohibit two rooms claiming one world

In v1.0.1: Local world locks prevent one-host concurrent writers but do not constrain backend references for the same world across rooms/devices.

Correction and required evidence: Race two room attachments and copied receipts for the same worldId across devices. Backend rejects duplicate active custody. An explicit fork gets new IDs and isolated mutable bytes; shared lineage does not grant shared write authority. See [04-ROOMS-AND-PERMISSIONS.md](04-ROOMS-AND-PERMISSIONS.md); primary acceptance is P07, A87.

### F36 — Expired maintenance barriers lacked a fenced recovery claim

In v1.0.1: The barrier correctly survived permit expiry, but no recovery-claim contract specified who could resume, how to fence stale workers or which actions were permitted.

Correction and required evidence: Expire a restore, crash/restart its agent, race two recovery claims and wake the old worker. One fenced recovery attempt reconciles the original decision; stale workers cannot publish/delete. Unknown living writers or revoked custody keep the barrier blocked. See [18-AUTHORITY-AND-MAINTENANCE.md](18-AUTHORITY-AND-MAINTENANCE.md); primary acceptance is P09, A88.

### F37 — Control deadlines could be starved or reused across a clock epoch

In v1.0.1: Conservative lease arithmetic did not specify control-worker isolation, total request deadlines or persistence limits of local monotonic timestamps.

Correction and required evidence: Saturate hashing/compression/HTTP queues, stall requests, restart agents, suspend hosts and inject backend clock rollback. Watchdogs expire access on time; stored local deadlines never create new authority and unhealthy clocks fail closed. See [03-OAUTH-AND-IDENTITY.md](03-OAUTH-AND-IDENTITY.md); primary acceptance is P11, A89.

### F38 — Ordinary audit retention could erase disaster-recovery authority

In v1.0.1: The 30-day audit default and generic result-cache retention did not specify a distinct horizon for non-rollback authority, unresolved custody and old restorable backups.

Correction and required evidence: Rotate ordinary logs, retain an older DB backup, delete a room during restore and attempt old retries. Minimal protected recovery records survive; insufficient authority history freezes restore and expired secret payloads are not retained. See [15-DEPLOYMENT.md](15-DEPLOYMENT.md); primary acceptance is P12, A90.

### F39 — Route policy was protected against PATCH but had no typed transition

In v1.0.1: The privacy UI described stricter changes, while protected-reference rules disallowed generic mutation and no valid operation implemented that change.

Correction and required evidence: Attempt policy changes during gameplay/pending start/maintenance; require controlled stop and revision-checked consent. The next start uses new endpoint contexts/revision and rejects old bindings; unchanged world data and honest prior-disclosure messaging are verified. See [04-ROOMS-AND-PERMISSIONS.md](04-ROOMS-AND-PERMISSIONS.md); primary acceptance is P07, A91.

## Remaining boundaries

The actual provider/repository mapping, game identity hook, release acknowledgment, server logging controls, selected transport behavior and OS/network matrix are still implementation gates. These requirements may expose an unsupported runtime tuple; the chatbot must report that instead of manufacturing a successful adapter. Added tests remain not_run.

This remains only Worlds with Friends. Own OAuth/room services, English repository artifacts, Russian development chat and authorized automatic Git workflow are unchanged. No previous launcher archive is required. The phase graph and original functional requirement IDs are unchanged.

## Document checks

The companion SPEC-AUDIT records structural validation: links, phase graph, case/requirement mapping, preserved original IDs, language constraints and ZIP byte correspondence. Presence checks for contracts are regression aids, not semantic proof or runtime tests. New requirements do not establish that any implementation is bug-free.
