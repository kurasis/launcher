# 10 — Room client/server generations

## Manifest

A room pack generation is a platform-neutral immutable payload: schemaVersion, gameVersion, runtimeFlavor, identityAdapterVersion, loader tuple, client/server references, supported target selectors, personal-content policy, provenance and creation time. Its SHA-256 is stored in a detached database/envelope reference, never as a required self-hash field inside the bytes being hashed. Hash the exact immutable payload bytes. Detached signatures cover those bytes/digest and do not recursively change it.

The room `packGeneration` is shared by Windows/macOS/Linux participants. It references exact artifacts or target-indexed immutable manifests for supported OS/architecture combinations. A local InstallLock additionally pins that device's Java archive, native libraries, paths and target-specific outputs. Do not compare a guest's platform-specific lock hash with the room's common generation hash. A missing target mapping is unsupported and is rejected before large downloads.

Each file entry includes normalized relative destination, side (`client`, `server` or `both`), required/optional status, size, digest, trusted source reference, origin project/version where available and redistribution policy. Runtime requirements are side-specific. The client and server are compatible plans, not necessarily identical file lists.

Core uses this contract for one qualified baseline generation, including any minimal identity bridge/bootstrap required by that tuple. P13 adds modpack import and publication. Unknown schema versions fail closed, not silently ignored.

## Join preparation

Read the session's selected generation and access revision. Resolve only immutable references; calculate download and extraction space, show executable-content consent, install missing verified objects and materialize an isolated room profile. Preserve personal graphics, controls and other explicitly allowed local settings outside mandatory content.

Before requesting admission, recheck the live session epoch and selected generation. If the room changed while downloading, retain cached bytes but do not launch the wrong profile; display the new plan and request any new consent. A digest supplied by the guest is readiness information, not cryptographic proof that its runtime cannot cheat.

Default core client profile is immutable for managed files. Never modify a user's unrelated instance to join a room. Optional personal mods require a published allow policy and cannot be assumed compatible.

## Mods and loader support

Use provider metadata and author-provided server packs first. Classify dependencies, loader compatibility and client/server environment from authoritative metadata where available. A filename or a successful client launch is not evidence of dedicated-server compatibility. Unknown side classification blocks automatic publication until reviewed or tested.

Fabric is the first modded target. NeoForge, Forge and Quilt each receive independent adapters, exact version tuples and real launch/identity tests. Compatibility with another loader is not assumed from marketing or source layout. Preserve processor/install outputs and required launch argument files for distributions that use installers; do not flatten them into an invented `java -jar` command.

Prefer official metadata/catalogs and provider-hosted downloads. Respect provider download restrictions; do not rehost all JARs through the relay or a pack ZIP. If redistribution is not authorized, export a manifest of permitted source references. A locally uploaded file needs explicit redistribution status; unknown means not redistributable by the platform.

## Update transaction

1. Publish an immutable candidate generation and diff.
2. Resolve and test the candidate against a disposable copy of a previously verified stopped-world snapshot. Do not copy a live working directory and call it a consistent test world.
3. Prepare guests' client artifacts without changing the live room generation.
4. Schedule transition; close admissions, stop server and create a verified final snapshot.
5. Record the pair `(old generation, compatible snapshot)` for rollback.
6. Apply the final snapshot to a fresh candidate and execute the maintenance commit protocol in [18](18-AUTHORITY-AND-MAINTENANCE.md) for world and pack references together. Start a new session and verify readiness before inviting clients.
7. If migration fails, stop the candidate and restore the matching snapshot before using the previous generation.

Rolling back files alone after a world-format change is not safe. Do not downgrade an already migrated working world. Conflicting configuration edits must produce a reviewable three-way diff or a clear manual conflict; never overwrite unknown user changes silently.

Pack signatures, when added, authenticate a publisher and immutable bytes; they do not certify that executable code is harmless. Consent and origin records remain required.

Qualification of an earlier snapshot does not guarantee migration of the final live data. Revalidate the final candidate before commit; if actual migration changes data, record the resulting new generation and preserve the final pre-migration snapshot. All changes to the room's selected world/pack pair require one maintenance operation. A metadata PATCH cannot bypass that transition.

Restore selection uses the snapshot-bound compatibility contract in [11](11-WORLDS-AND-BACKUPS.md). A newer currently selected room pack does not override the snapshot's required pack. World data and executable generation are committed as one compatible pair.
