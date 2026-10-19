# 11 — World custody, snapshots and restore

## World identity and custody

A world has worldId, lineageId, current generation, designated host, last clean session, pack generation, previous snapshot and mutable-data policy. The authoritative working directory includes the world plus server configs and mod-owned mutable state required to reproduce it, subject to the portable-data and logging exclusions below. Do not assume every mod saves exclusively inside a folder named world.

Core custody is local. A room record is not a backup. Snapshot status explicitly says local-only until remote storage is implemented and verified. A host's filesystem access inherently lets its owner read and modify the world; the platform cannot prevent that with room roles.

## Import

Import into a new isolated directory. Require the source game/server to be stopped and its source lock/custody to be established, or import an already verified immutable snapshot. Merely copying a directory while another process writes it is not a consistent import. If the source cannot be proven stable, refuse a production import and offer an explicitly unverified forensic copy for later recovery; never label it ready. Reject traversal, absolute paths, drive/UNC paths, ambiguous separators, escaping links and archive bombs. Show required space and selected content before extraction. Validate layout, game version and known mod dependencies. Preserve unknown files until an adapter classifies them.

For singleplayer import, copy first and run a validation launch. Verify the owner's inventory/profile mapping, dimensions, advancements, datapacks and mod state through the identity adapter. Report a profile-mapping problem rather than silently inventing a new player identity. Keep the original untouched.

## Core snapshot algorithm

Core supports stopped-world snapshots; core export also uses stopped-room maintenance as specified in [18](18-AUTHORITY-AND-MAINTENANCE.md). Acquire a backend maintenance permit before changing room availability. The per-world actor notifies players, closes new admissions and stops its own Java child, then retains the same world reservation while snapshotting the complete mutable-data set. It does not wait for a second world lock held by that child. This deliberately avoids claiming that copying a live directory is consistent.

The manifest lists every included relative path, type, size and SHA-256, source world/lineage generation, game/server/loader locks, identity adapter version, snapshot format version, consistency=`clean_stopped`, creation time and exclusions. Include actual mutable configs and mod data; exclude credentials, process locks, transient logs and reproducible managed cache only through a versioned adapter policy. Nonportable external paths require explicit handling and cannot be silently omitted.

Write to private staging, verify all payload digests and archive readability, then flush. Prepare and commit the non-pointer result under [18](18-AUTHORITY-AND-MAINTENANCE.md) before atomically publishing its local completion receipt and acknowledging it. Do not mark success before verification and reconciled publication. Retain the source lock until the operation reaches its terminal custody boundary. A failed snapshot leaves the previous valid snapshot intact and reports whether the server is stopped.

Before a manual snapshot of a running room, show that players will disconnect and offer an explicit restart-after-success option. Restart creates a fresh session/epoch and fresh admission grants. Do not silently resume an old closed session. Failed snapshots remain stopped unless the user deliberately chooses a safe restart of the unchanged working world.

Optional later hot snapshots require a qualified adapter with an actual save barrier and a consistent snapshot mechanism for every participating storage subsystem. Filesystem copy-on-write alone does not prove application consistency.

## Retention and space

Default local policy: up to 5 successful automatic snapshots and a configurable byte budget; keep the newest valid recovery snapshot even if it exceeds the automatic budget, then pause new automatic snapshots and explain the shortage. Manual pinned snapshots are not silently deleted. Delete old eligible snapshots only after a newer snapshot commits, and never delete an object referenced by an active restore/export/handoff.

Estimate peak space for working data, snapshot staging, extraction, restore candidate and rollback reserve. If insufficient, stop before mutating the authoritative world. Cancellation removes only owned staging and never the last valid snapshot.

## Restore transaction

Require current owner/delegated permission, device consent and confirmation naming snapshotId and digest. Allocate a restore maintenance operation as defined in [18](18-AUTHORITY-AND-MAINTENANCE.md), recheck authority through the operation lease, and have the lifecycle actor stop Java while retaining the world reservation.

Verify the snapshot, source provenance, required pack/runtime availability and all extracted files in a new candidate generation. Create a second disposable validation clone of that candidate. Launch only this clone under an isolated qualification purpose with private listeners, no room publication, no guest admissions and its own process receipt; suppress external integrations through the qualified adapter or obtain explicit consent for unavoidable side effects. Tests are not a sandbox for arbitrary trusted executable code.

Stop the validation child cleanly and discard its clone. The restore candidate remains byte-identical to the verified snapshot payload, except separately documented regenerated local runtime overlays; a validation tick/save must not advance the candidate's world. A required intentional migration is a distinct recorded operation, not an invisible part of restore.

Commit the new local generation plus backend placement through the recoverable protocol in [18](18-AUTHORITY-AND-MAINTENANCE.md). Preserve the old generation for rollback. A failure before commit leaves the previous active pointer unchanged. A failure after backend commit enters reconciliation and cannot pretend the prior world is still authoritative. No gameplay starts until backend and local receipts agree.

## Export and deletion

Export includes world data and a reproducibility manifest but excludes credentials, invites, device keys, local absolute paths and other users' private diagnostics. Explain missing nonredistributable artifacts. A room deletion does not implicitly delete its world. Local deletion uses a recoverable trash transition where practical and refuses active world directories.

## Recovery

After an unclean exit, preserve files and mark consistency unknown. Offer a copied validation attempt or restore from a verified snapshot. An emergency copy may be retained as forensic data with consistency=`unclean`; it must not be presented as a validated snapshot. Do not repeatedly auto-restart a server that may be modifying damaged data.

## Runtime overlays and portable data

Separate portable world/config data from per-host runtime overlays such as management credentials, listener addresses, device bindings and external service secrets. Snapshot manifests identify overlay exclusions and the adapter version that regenerates them on the destination. Do not hash a secret-bearing runtime overlay into a portable manifest or export it to guests.

Adapters classify sensitive fields and required external storage. Unknown mod-owned files cannot simultaneously be promised complete and secret-free: mark export/portability unsupported until their policy is reviewed, or require explicit reviewed inclusion. Missing excluded external state must block a claim of reproducible restore. Snapshot bytes remain local in core and never pass through Room API event payloads.

Default automatic snapshot trigger: after a clean stop, if enabled, while the lifecycle actor retains custody; before a risky pack transition/restore, require a valid rollback snapshot or an explicit acknowledged recovery exception. Core has no periodic live-copy backup. Show automatic snapshot failure and whether the prior working generation is still available.

## Restore compatibility and identity provenance

A restore is bound to the snapshot's exact packGeneration, world format, runtime flavor and identity-mapping contract. Compare these with the current room before stopping it. Core restores only its qualified baseline tuple; an unavailable artifact or unsupported tuple returns RESTORE_TUPLE_UNAVAILABLE before destructive work. Do not silently substitute the current pack, a newer loader or a newly resolved runtime. Once P13 permits pack transitions, restoring an older snapshot commits the matching world/pack pair together with fresh executable consent where needed. Validation of a clone cannot justify opening the unchanged candidate under a different pack.

The identity adapter freezes a mapping namespace/version for persistent player data. Renaming an account must not change its stable gameProfileId; different accounts cannot claim the same profile inside that namespace. Deleted/recreated accounts must not automatically inherit old player data through a reused display name. Snapshot provenance contains the non-secret mapping contract reference, not account credentials. Cross-namespace import or a changed mapping requires an explicit reviewed copied-data migration; uncertain mappings block readiness instead of silently creating empty inventories. Restoring files never restores old room membership or grants.

For a failure before backend commit the old pointer remains authoritative. For a failure after commit, preserve both generations and remain reconciling; only the committed candidate or an explicitly authorized compensating transition may become ready. Peak-space estimates include the second full validation clone when reflinks are unavailable.
