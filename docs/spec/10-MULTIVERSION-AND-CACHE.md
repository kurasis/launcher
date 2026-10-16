# 10 — Multiple versions, shared files and failure scenarios

Specification version: **1.2.1**. This document is normative and refines documents 01–08. It does not extend the supported version set beyond the two initial qualification candidates.

## Required behavior

Create A using 1.21.1, B using 26.3, and A2 using 1.21.1. Each has its own instance UUID and game directory. A and A2 may share the exact client/libraries/assets/runtime but must not share worlds, options, run directories or mutable user data. A and B share only matching verified content and use their own pinned runtime requirements. A user's display name for an instance is never a filesystem key.

Selecting an instance selects its immutable version/descriptor and runtime lock. It does not rewrite another instance or choose whichever Java executable was used most recently. One global activity permit allows one install/repair OR one game/preflight/cleanup; this restriction is deliberate for MVP. Multiple installed versions do not mean multiple concurrent Java games.

## Identity and storage contract

| Object | Identity / reuse rule | Canonical location or ownership |
| --- | --- | --- |
| Instance | Generated UUID | Private instance.json and game directory |
| Version descriptor | Version ID and raw descriptor SHA-1 | Immutable metadata bytes; different hashes remain distinct |
| Client JAR | Client SHA-1 plus validated expected size | `shared/clients/<sha1>/client.jar`; independent of version ID |
| Library or native JAR | Artifact SHA-1 plus validated expected size | `shared/libraries/<sha1>/artifact.jar`; independent of Maven coordinate/version |
| Asset object | Asset SHA-1 plus validated expected size | Mojang-compatible `shared/assets/objects/<prefix>/<sha1>` |
| Asset-index source | Raw index SHA-1 | `metadata/asset-indexes/<sha1>.json` |
| Asset-index projection | Index ID plus currently selected instance's locked source hash | Mutable game-facing `shared/assets/indexes/<id>.json`, rebuilt under the activity permit |
| Logging configuration | SHA-1 and safe declared ID | Shared hash-addressed configuration; no user edits |
| Java installation | Provider, OS, architecture, major, image type and archive SHA-256 | `runtimes/temurin/<os>-<arch>/<major>/<jre-or-jdk>/<sha256>/` |
| Temporary/native extraction | Generated run UUID | Private run directory; never shared between live runs |
| Worlds/options/screenshots | Instance UUID | Private game directory; never a shared-cache entry |

Digest strings are normalized lowercase and validated for their algorithm's exact length. Expected size is a consistency condition, not a way to accept two different files under one digest. If two records for the same artifact category/algorithm/digest disagree on size, stop with METADATA_CONFLICT. Do not use a URL, basename, Maven version or release label as the verified-content identity. Same filename with different digests means separate content; different Maven coordinates with the same validated digest reuse one library object. Categories retain their documented canonical layout; the MVP does not promise physical deduplication of a hypothetical byte-identical object across unrelated categories.

Keep original Maven coordinates, declared relative paths, source URLs and artifact roles in the lock for provenance and future adapters. Runtime release names also remain provenance. The initial vanilla versions use the explicit ordered classpath. Preserve its order and reference exact canonical JAR paths; do not rely on filenames, directory globbing, a latest-library map or a single conventional Maven root. Required library_directory semantics in a new metadata format need a separately tested adapter.

## Sharing is measured, not inferred

For a second install, compute the required set for its target, validate reusable objects, then fetch only missing/invalid content. A valid object is reusable even when the new descriptor gives a different allowed source URL. Deduplicate concurrent requests within the one active job. Persist receipts after atomic commit and before counting content as reusable.

Network transfer reuse and physical disk reuse are separate measurements. Client/library/asset canonical paths above are shared directly; there is no per-instance copy of those bytes. Each instance still has its own small metadata lock and mutable game files. Java's extracted tree is shared only for an identical runtime key. Different majors, JRE versus JDK, or different archive digests coexist; never overwrite one because the release name matches.

Game-owned caches and native extraction may duplicate data across instances/runs. They are not covered by a promise of perfect whole-disk deduplication. No automatic cache garbage collector is part of MVP. A later deletion/GC feature must calculate references from all live install locks and active plans before deleting shared content.

## Mutable index projection is not corrupt content

If A and B use the same asset-index ID but different locked hashes, each immutable source remains valid. The single game-facing index path can represent only the selected run. A different current projection is normal, not proof that another installation is damaged.

Preflight first validates immutable sources, then writes the selected projection under the global activity permit before spawning Java. Rebuilding that derived projection from already verified local bytes does not require Repair or a network request. A matching projection may be reused. Do not include this mutable alias in shared-corruption invalidation or treat its current hash as the health of every instance. If the immutable source itself is missing/corrupt, normal Repair rules apply.

## Commit and recovery rules

1. Acquire the supervisor's DATA_ROOT lifetime lock on startup and the activity permit for each install/repair/run. UI single-instance handling does not replace these locks.
2. Resolve a new installation once and persist the final plan in its job receipt before downloads. Retry keeps its descriptor/runtime selection. An existing completed install lock is never silently replaced by fresh latest metadata.
3. Commit verified artifacts and runtime tree receipts first, the completed install.lock.json second, and the derived instance summary last. Restore a stale summary from a valid lock after a crash; never trust ready without the lock.
4. Shared corruption invalidates the object receipt and derived health of all referencing instances, while immutable locks retain expected hashes. Repair once can fix that shared object for every instance. No game may read it while replacement is in progress.
5. Failed/cancelled Repair does not prove readiness. Preserve the completed historical lock and use needs_verification/repair_required until checks finish. A projection belonging to another instance is handled by preflight, not this corruption path.
6. After unexpected supervisor death, reconcile any surviving game before allowing writes or another run. Mere absence of the old supervisor or deletion of a PID file is not sufficient.

## Regression scenarios

These are required implementation checks, not tests already executed against an application. Fixture tests may validate metadata/cache decisions; actual game tests remain required for the supported platform matrix.

| ID | Setup and action | Expected result | Phase ownership |
| --- | --- | --- | --- |
| MV01 | Install A, then A2 with the same descriptor and exact runtime receipt. Count artifact HTTP bytes and inspect paths. | No duplicate artifact transfer or per-instance client/library/asset/runtime copy; worlds/options remain separate. Metadata/auth requests are counted separately. | P05–P08, P13 |
| MV02 | Install A and B; alternate real play/save/exit. | Each launches its own client/classpath/Java major and retains its own world/options; no auto-upgrade or last-used-Java substitution. | P08–P10, P13 |
| MV03 | Fixture: same Maven coordinate/path or client version label, different digests. | Distinct canonical content; no overwrite by filename/coordinate. Classpath/lock selects the intended digest. | P05, P06 |
| MV04 | Fixture: different library coordinates/URLs, identical digest and size; then conflicting size for that digest. | Reuse the verified library once in the first case; METADATA_CONFLICT in the second. Preserve ordered references. | P05, P06 |
| MV05 | Resolve JRE and JDK with the same release label, and two package digests for the same platform/major. | Separate runtime keys/directories; existing instance runtime remains pinned. Retry does not fetch a fresh latest build. | P07, P08 |
| MV06 | Fixture: A/B share an asset-index ID but pin different source hashes; switch launches. | Correct projection before each spawn, no false shared-corruption or forced Repair; immutable index sources remain untouched. | P05, P08, P09 |
| MV07 | Corrupt one canonical library used by A/A2, then Repair A while idle. | Both are prevented from using stale health; one verified replacement restores the content for both; worlds stay byte-identical. Repair while B runs returns APP_BUSY. | P08, P09, P13 |
| MV08 | Interrupt after artifact commit, after install-lock commit and before summary update; also interrupt Repair. | Verified content reused, stale summary reconciled, interrupted Repair never falsely ready, exact resolved runtime retained. | P02, P08 |
| MV09 | Start two launcher processes/sessions against the same DATA_ROOT. | One writer/supervisor owns the lifetime lock. Failure to signal/focus it does not authorize the other to write. | P02, P03, P13 |
| MV10 | Close GUI during login, preflight and install; finish/fail background install; use cancel-and-exit. | Login/pre-spawn cancellation is safe; install success/failure restores one result window; explicit cancel-and-exit does not reopen it. No hidden idle process is stranded. | P04, P09, P10 |
| MV11 | Fill or make the launcher log destination unwritable during heavy synthetic child output. | Child pipes remain drained, buffers remain bounded, no raw fallback log, logging-degraded outcome shown; normal game supervision continues. | P09, P10, P13 |
| MV12 | Use a root containing the platform classpath delimiter, and supported Unicode/space paths; race cancel with spawn. | Unsupported roots fail before bulk download; supported paths work with real Java. Cancel before spawn creates no game; cancel after spawn requires explicit Stop confirmation. | P02, P08–P10, P13 |

## Concrete metadata comparison

The following comparison used the two retained, hash-verified descriptors identified in document 09 and their hash-verified asset indexes. Library rules were evaluated for Windows x64; these counts are not a claim about every OS. Asset objects were deduplicated by digest before counting.

| Measurement | Result |
| --- | --- |
| Unique applicable library objects, 1.21.1 / 26.3 | 70 / 74 |
| Matching library digests between the two | 4, totaling 183,250 declared bytes |
| Unique asset objects, 1.21.1 / 26.3 | 3,888 / 5,147 |
| Matching asset digests between the two | 192, totaling 102,368,477 declared bytes |
| Client digest equal | No; keep separate clients |
| Required Java majors | 21 / 25; keep separate runtimes |

This verifies that metadata contains reusable content and distinct runtime requirements. It does not measure application download savings, validate every referenced binary, or prove a game launch. Shared-byte totals exclude metadata, logging configuration, runtime packages and transfer overhead. Changes to descriptors/indexes can change the counts. Both raw asset indexes were fetched/hashed during this review; game binaries were not downloaded.

## Existing implementation transition

If development already used the 1.2.0 layout, discover old managed artifacts only through validated old locks/receipts. Under exclusive activity, hash them against expected content and populate the new canonical locations by a safe local copy or an appropriate verified same-filesystem operation. Do not redownload valid content solely because its pathname changed. Commit new references only after their targets exist; retain old paths until no active/new lock depends on them. No whole-directory reset, world migration or destructive cache cleanup is authorized by this specification update.
