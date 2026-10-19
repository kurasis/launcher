# 04 — Rooms, membership and authority

## Model

A room persists independently of its sessions. It references one active world lineage, one selected immutable pack generation, a designated host device and membership policy. A session is one attempt to run that room on an authorized host. A room may have historical sessions, but at most one live authoritative session.

Room fields: ID, owner account, name (1–64 Unicode characters), description (0–1000), optional validated cover asset ID, visibility=`private`, selected worldId, selected packGeneration, designated deviceId, revision, accessRevision, ownerRevision, executionRevision, routePolicy, routePolicyRevision, worldState, createdAt, updatedAt and deletedAt. Session fields include sessionId, roomId, hostDeviceId, monotonic hostEpoch, build locks, state, lease expiry, world generation, startup timestamps and terminal outcome.

Membership is unique on `(roomId, accountId)`. An owner is required. Device ownership is not room ownership. A room manager cannot implicitly control every device owned by the room owner.

## Permissions

| Action | Owner | Manager | Member |
| --- | --- | --- | --- |
| View room and join | Yes | Yes | Yes |
| Create ordinary member invitations | Yes | Yes | No |
| Remove ordinary members | Yes | Yes | No |
| Change managers or ownership | Yes | No | No |
| Start/stop assigned host | Yes, subject to device consent | Only delegated host capability | No by default |
| Change world/pack or restore snapshot | Yes | Explicit delegated capability | No |
| Read redacted operational diagnostics | Yes | Delegated | Own connection only |
| Delete room or configure billing | Yes | No | No |

Host-device consent is separate: the device owner chooses allowed rooms, operations, resource limits and expiry. Backend permission plus device consent are both required. Arbitrary terminal/console access is not a core permission.

## Invitations

Use a CSPRNG token with at least 128 bits of entropy; the invitation lookup record stores only its digest. Any exact retry response is separately encrypted and expires under the bounded secret-result policy in [09](09-API-AND-IPC.md). Default expiry 24 hours, maximum 7 days, default use limit 1. An invite can optionally bind an intended account and require owner approval. Invites grant member status only. Only an owner-authorized, revision-checked membership operation may elevate an active member; issuing another invitation cannot grant a manager role.

Use an HTTPS landing link with its secret in the fragment; the controlled landing page clears it promptly and never sends it to analytics or third-party assets. A desktop link may carry it only in a bounded structured parameter, with local log redaction. The user must confirm redemption. Invitation tokens are not OAuth credentials, host addresses or permission to execute downloaded code.

Redeem in a database transaction: verify active invite, room state and recipient; enforce expiry and max uses; create pending/active membership; consume a use only for a new membership request. An existing membership retry must not burn additional uses. Approval must recheck current room policy. Concurrent last-use redemption admits at most one new account. Denied or banned accounts cannot self-rejoin by redeeming another token.

## Session lifecycle

The room is `idle` when no active session exists and its world is initialized; uninitialized or reconciling worlds have separate availability states. A created session moves `preparing -> starting -> ready -> stopping -> stopped`. Any active session may enter `recovering`, `failed` or `lease_lost`; `recovering -> ready` is allowed only for the same verified live process under still-valid authority. A lost/expired lease is terminal for that session and cannot be revived by a delayed heartbeat. A restart after actual Java exit creates a new session ID and new grants. Tunnel reconnect while the same Java process lives does not create a new world/session generation.

Readiness requires: live unexpired host lease, exact installed lock, server protocol readiness, identity adapter readiness and accepting host-agent admission endpoint. A log substring alone is not enough.

Room listing distinguishes `host_offline`, `host_available`, `preparing`, `ready`, `degraded`, `stopping` and `unknown`. Never show stale presence as proof of joinability. Events are hints; a revisioned snapshot is authoritative.

## Host lease and fencing

Acquire a host lease transactionally with a new strictly increasing epoch within the current authorityEpoch; unique constraints prohibit two live sessions per room. Backend restarts must not confuse an expired lease with proof that the previous Java process has stopped. Persistent placement and maintenance rules are in [18](18-AUTHORITY-AND-MAINTENANCE.md). Renew with exact session/device/epoch. Expired or superseded hosts cannot receive new admission grants. Every privileged command carries the expected epoch and revision.

The cooperating host closes tunnels on lease expiry and requests graceful server shutdown. A backend lease cannot physically stop an unreachable or malicious computer from editing a local copy. Do not promote another authoritative host until stopped-world authority transfer is established; P16 defines conflict quarantine and fencing. No automatic failover to an old snapshot in core.

## Deletion and ownership transfer

Deleting a room closes admissions, revokes invites and requests session stop. Metadata deletion and local world deletion are separate explicit actions. Retain a tombstone for delayed commands; do not erase world bytes merely because a user left a room.

Owner transfer requires current owner authorization, recipient acceptance, active membership and a revision-checked transaction. It changes account authority, not filesystem custody; moving the world is a separate P16 operation.

## Membership and transfer edge cases

Membership states are pending, active, denied, left, removed and banned. Only active members receive gameplay grants or private room details. A normal removal may be followed by a new permitted invitation; a ban/denial requires an owner-authorized clear action before reapplication. Removing the last owner or changing the owner through a generic member PATCH is forbidden. A member may leave, but an owner must transfer ownership or explicitly delete the room first.

Owner transfer is allowed only while the room is stopped with no pending maintenance/handoff. Acceptance checks both revisions and the current owner, increments ownerRevision and accessRevision, invalidates outstanding invites/delegated commands, and clears delegations tied to the previous owner. Host consent is bound to ownerRevision: the old host must explicitly re-consent before hosting for the new owner. Transfer does not convey filesystem custody or automatically grant the new owner export rights to that device.

In core, start/restore/import/export originate on the assigned local host with current account authorization and local consent. Schemas may reserve remote execution fields, but remote UI/delegation remains disabled until P15. The physical device owner always retains the ability to stop local resource use; this does not authorize changing room ownership or exporting another user's data through platform APIs.

## Protected room references

Generic metadata PATCH cannot mutate active worldId, world generation, selected pack generation, designated host or route policy. Those fields change only through the typed authority operations in [18](18-AUTHORITY-AND-MAINTENANCE.md). Core room creation selects a curated supportTupleId and validated world configuration; the backend creates the immutable baseline manifest. A host cannot publish arbitrary executable manifests under a normal room metadata update.

## World reference uniqueness

At most one non-deleted room may own the active custody reference for a worldId. Enforce this in backend constraints and transition transactions, not only through a local filesystem lock. The world has one current placement across devices. A worldGenerationId is scoped to that world and cannot be attached to an unrelated room by accepting an arbitrary client ID. A historical shared lineage is allowed, but an explicit copied-world fork allocates a new worldId and generation IDs; it does not share a mutable working path or transfer current authority. Snapshot references can be shared only with explicit read authorization and immutable pins.

Room creation, initial publication, restore, deletion and future handoff enforce this invariant. A tombstoned room retains history/custody records until reconciliation; reusing its world for a new room requires an explicit authorized attach/fork decision, never an automatic shortcut around a stop or deletion fence.

## Route-policy transition

Core changes `routePolicy` only while stopped, with no pending start or maintenance operation, through POST `/v1/rooms/{id}/route-policy-transitions`. Require current owner authorization, expected room/owner/execution revisions and explicit host-device consent to the new policy. A running-room UI first offers a controlled stop; core does not promise an in-place privacy upgrade. This metadata-only CAS increments roomRevision, executionRevision and routePolicyRevision, invalidates prior endpoint bindings and queued joins, and preserves world bytes/placement.

The next start and guest join register fresh contexts bound to the new routePolicyRevision. Old endpoint records cannot be re-enrolled under the new policy by an idempotent retry. Scope tickets/discovery to this revision. A route label is not evidence of enforcement; [07](07-TUNNEL-PROTOCOL.md) and packet capture still gate support. Changing policy cannot erase an address previously disclosed. General metadata PATCH continues to reject route-policy writes.
