# Second specification review — 1.2.1

Date: 2026-10-17 (Asia/Dubai). Reviewed input: specification 1.2.0. The review focused on execution scenarios after the user's questions about multiple game versions and shared files. The architecture remains Tauri 2 + React + TypeScript + Vite with a Rust core.

The previous version was not completely precise. The findings below were corrected in the normative documents and relevant phase checks. This review is a specification walkthrough supported by metadata comparison and archive validation, not a runtime audit of an implemented launcher.

## Corrections

| ID | Finding in 1.2.0 | Resolution in 1.2.1 |
| --- | --- | --- |
| R2-01 | Runtime directory used platform/major/release but omitted JRE versus JDK and archive digest, permitting two different packages to collide. | Include image type and archive SHA-256 in the runtime key/path; preserve release as provenance. MV05. |
| R2-02 | Client paths included version ID and library paths included Maven path, so equal content could still get duplicated across versions/coordinates despite the reuse promise. | Canonical client/library paths use their content hash; keep version/Maven identities in locks. Add explicit reuse-byte and disk-path checks. MV01, MV03, MV04. |
| R2-03 | Two independently atomic JSON writes did not specify which committed installation state wins after a crash. Interrupted retries could resolve a new runtime. | Install lock is authoritative; instance summary is derived. Persist the resolved job plan before bulk downloads and preserve it on retry. MV08. |
| R2-04 | Single-instance UX and PID records did not explicitly require an OS-backed data-root writer lock. | Add lifetime DATA_ROOT lock; a failed notification or another desktop session never permits a second writer. MV09. |
| R2-05 | Shared repair did not specify invalidation for other referencing instances; mutable asset-index projection could be mistaken for corrupt immutable content. | Separate derived health and mutable projection from immutable content. Reproject locally before spawn; invalidate only actual shared-object corruption. MV06, MV07. |
| R2-06 | Background install completion, GUI close during login/preflight, and cancel racing spawn lacked complete outcomes. | Extend lifecycle table and cancellation commit boundary; background install success/failure restores GUI, explicit cancel-and-exit does not. MV10, MV12. |
| R2-07 | A failed log sink could stop pipe readers; the two-second pipe-drain allowance also conflicted with the unconditional two-second UI target. | Keep bounded draining despite sink failure; separate normal return target from bounded-drain recovery target. MV11. |
| R2-08 | Java argfile quoting does not resolve classpath delimiters embedded in a path, yet no early path check was specified. | Validate DATA_ROOT before downloads; document the narrow unsupported-path case and test supported Unicode/space paths. MV12. |
| R2-09 | Interrupted operation outcome was missing from the example type; health and operation state could be conflated; requestId requirement disagreed with command-table payloads. | Add interrupted state, separate InstallationHealth, define the common MutationRequest envelope and cancellation-by-kind behavior. Synchronize auth cancellation/sign-out wording. |

## Scope that remains deliberate

- Multiple installed instances/versions are required, but only one game can run at a time.
- Initially qualify 1.21.1 and 26.3; arbitrary legacy versions, snapshots, inherited/modded metadata and loaders remain out of MVP.
- Assets/libraries/client bytes are shared when their validated identities match. Different Java majors/packages coexist. Worlds and user settings remain isolated.
- No public release bypasses own-client auth, current entitlement checks, integrity checks or the real target-machine qualification matrix.

## Evidence and validation

The two retained version descriptors and both referenced asset indexes were hash-checked and compared. Windows x64 applicable libraries have four shared digests; asset indexes have 192 shared object digests. The exact counts, declared bytes and limits are in [document 10](10-MULTIVERSION-AND-CACHE.md). This is metadata evidence, not a real installer test.

Packaging validation checks all delivered Markdown files for readable UTF-8, balanced code fences, valid relative file links, the P01–P14 dependency graph, the existing FR01–FR16/A01–A26 identifiers and MV01–MV12 coverage. It verifies that original documents are retained, then performs ZIP CRC and byte-for-byte readback checks. The archive contains 31 Markdown files and no credentials, downloaded game binaries, runtime archives or research scratch files. The first audit report is retained with an explicit historical label; its earlier file counts describe version 1.2.0.

## Assessment

The corrected document is a concrete implementation brief with explicit scope, source inputs, storage ownership, stage dependencies and observable acceptance criteria. No further blocking contradiction was identified in the reviewed scenarios after these changes. That statement is limited to this review: actual auth service access, binary compatibility, platform lifecycle and package behavior must still be demonstrated by the implementation. It would be inaccurate to claim that a document review proves the future launcher bug-free or production-ready.

Implement the early vertical slice first: P04 auth, P08 install, P09 actual launch and P10 return, while preserving their prerequisites. The MV cases are refinements of the existing phases, not an additional large subsystem or a reason to postpone a working MVP indefinitely.
