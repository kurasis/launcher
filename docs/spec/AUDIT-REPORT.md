> Historical report for version 1.2.0. The current specification is 1.2.1; see [SECOND-AUDIT.md](SECOND-AUDIT.md) for the subsequent corrections and current package validation. Counts below refer to the earlier archive.

# Full Markdown archive audit — version 1.2.0

Audit date: **2026-10-17 (Asia/Dubai)**. Input: `minecraft-launcher-mvp-spec.zip`, specification 1.1. Output: `minecraft-launcher-mvp-spec-v1.2.0.zip`.

Input archive SHA-256: `1838e6b72ce093a7349340eb918fede7e2045bec552d6205c479fd5ec49d6cdf`.

## Scope and result

Reviewed all **27 original Markdown files**, including all P01–P14 phase files. Their bytes matched the input archive before editing. The revision changes all 27 and adds this report plus CHANGELOG.md, producing **29 Markdown files**. The previous archive remains a separate artifact.

The audit found requirement contradictions, incomplete contracts and implementation hazards. The corrections below are changes to the specification, not claims that bugs in an existing application were reproduced or repaired. No application source or built launcher was supplied for this audit.

Severity means likely implementation impact: **High** can block a core journey, compromise credential handling or allow incompatible shared-data mutation; **Medium** can create incorrect behavior, misleading state or qualification gaps. Each finding is resolved at specification level and has corresponding implementation checks. This is not a guarantee that no further defects will be found while implementing the integrations.

## Findings and corrections

| ID | Severity | Original problem and likely effect | Correction and implementation evidence required |
| --- | --- | --- | --- |
| AUD-01 | High | P04 completion required the full live account matrix while PHASES allowed P09 despite P04/G0 blockers; dependency semantics disagreed. | P04 requires real positive own-client auth plus policy fixtures. Full real account matrix remains mandatory at P13/G5. P09 has no dependency exception. See 03, 08, PHASES, P04, P13. |
| AUD-02 | High | P05 promised a complete plan before its P07 runtime resolver existed; byte estimates could omit Java. | GamePlan + RuntimeRequirement in P05, RuntimePlan in P07, composition in P08; runtime totals remain unknown until resolved. Test deterministic plans from fixed inputs. |
| AUD-03 | High | “All JSON” required an app schema, conflicting with hashes over raw Mojang metadata. | App-owned documents alone receive schemaVersion; upstream bytes remain unchanged with separate receipts. Hash the original bytes and test exact preservation. See 02. |
| AUD-04 | Medium | Instance creation required a descriptor hash before the catalog/resolver phase. | Draft instances use null until verified resolution; ready locks require non-null pinned hashes. Test create/restart/resolve. See 02 and P02. |
| AUD-05 | High | Per-instance locks allowed another instance's Repair to replace shared runtime or asset files during a game. | One global mutation-or-game permit, including cleanup and surviving orphan protection. Test cross-instance conflicts and deterministic lock order. See 02, 04, 05, P08–P10. |
| AUD-06 | High | Global asset-index paths use IDs that may point to different content hashes in different locks. | Project the selected locked bytes immediately before spawn under the global permit; forbid concurrent replacement. Test alternating two same-ID/different-hash instances. See 02 and 08. |
| AUD-07 | High | Archive SHA-256 did not define verification of extracted Java files; Repair could miss corruption and execute it first. | Extracted-tree receipts cover files, modes and internal links; rebuild a corrupt pinned runtime in staging before execution. Test corrupt Java and library entries. See 04 and P07. |
| AUD-08 | High | Repair/maintenance wording allowed implicit descriptor or Java updates despite immutable instance requirements. | MVP Repair restores exact locked inputs; new runtime patch/metadata upgrades are deferred. Missing pinned artifacts produce errors, not substitutions. See 01 and 04. |
| AUD-09 | Medium | Atomic file/directory replacement was described without cross-platform recovery behavior. | Specify Windows-tested file replacement and recoverable nonempty-runtime directory promotion without deleting last-good content first. Test interruption at commit boundaries. See 02 and 04. |
| AUD-10 | High | Inherited JVM environment variables could inject flags, conflicting heap values or agents into managed Java. | Filter the named JVM option/classpath variables for probes and game; preserve essential desktop environment. Test with hostile/synthetic option values. See 05 and P09. |
| AUD-11 | High | Argfile guidance omitted native encoding and permitted token-bearing files without a necessary MVP use case. | Non-secret JVM/classpath argfile only; direct game arguments; actual Java 21/25 encoding/escaping tests, explicit unsupported-path error. See 03 and 05. |
| AUD-12 | Medium | Optional auto-reopen and detach choices conflicted with mandatory return and an otherwise small MVP. | Two UI modes with guaranteed result handling; no auto-reopen toggle and no intentional detach. Explicit Keep background or Stop and quit. See 05, 06, P10, P11. |
| AUD-13 | High | Exit interception and window races lacked concrete native-Quit and delayed-destroy cases. | Central quit intent, main-thread serialized window control, tested pinned Tauri APIs, no universal prevent_exit assumption. Test close/menu/keyboard/reopen/exit races. See 05 and P03. |
| AUD-14 | Medium | Per-run logging caps did not bound old runs; post-exit pipe EOF could wait indefinitely. | Total retention and per-run caps, bounded tails/lines, bounded two-second final drain and end-of-stream redaction. Test inherited pipe handles and split secrets. See 05. |
| AUD-15 | High | Cancel/sign-out and refresh races could restore stale identity; failed vault deletion could be reported as successful sign-out. | Account generation, serialized refresh, busy policy, sign-out-pending marker and deletion retry before credential restoration. Test late completion and restart after vault failure. See 03, 06, P04. |
| AUD-16 | Medium | Refusal guidance could classify unknown 403 as missing approval; external browser closure was treated as directly observable. | Specific cause only from recognized diagnostics; generic forbidden otherwise. Browser close resolves through Cancel/timeout. Test unknown restriction and absent callback. See 03. |
| AUD-17 | Medium | Loopback callback instructions did not distinguish localhost and IP-literal registration/port behavior or invalid-request consumption. | Freeze the verified redirect profile; reject unrelated callbacks without consuming the active attempt; constrain listener/resource lifetime. See 03 and Microsoft source in 09. |
| AUD-18 | Medium | Important Xbox request fields were implicit, and copying reference diagnostics could leak auth payloads. | Structured request-field contract, HTTPS-only credential traffic, no redirects/raw auth logs, conservative one-use-code retry. Validate against own registration. See 03. |
| AUD-19 | High | “Exactly-once completion” could be misread as guaranteed event delivery to a destroyed webview. | Durable terminal outcomes, best-effort events, epoch/revision bootstrap, gap resync and request-ID deduplication. Test lost replies, reconnect and backend restart. See 06 and P06. |
| AUD-20 | Medium | Protocol/sign-out/interrupted states and shared-busy/repair errors lacked a consistent UI contract. | Added typed state/error cases; install progress is distinct from auth/run states; log cursors are independent. Test DTO consistency and recovery actions. See 06. |
| AUD-21 | Medium | RAM/resolution were “validated” without concrete common bounds. | Defined Rust-owned ranges and defaults semantics with physical-memory reserve; UI mirrors the same policy. Test boundaries and insufficient-memory state. See 06. |
| AUD-22 | High | Only qualified versions could run, yet qualification required first running candidate versions and packages. | Explicit private qualification builds use fixed candidates with unchanged auth/integrity gates. P13 qualifies, rebuilds policy-only final artifacts and smokes those exact artifacts. See 01, 07, P12, P13. |
| AUD-23 | Medium | Ubuntu 22.04+ wording suggested runtime coverage beyond the stated test distributions; app identity migration was implicit. | Advertise tested Ubuntu 24.04/Debian 12 only initially; record exact other OS builds. Choose stable identity before distribution and test data preservation. See 01 and 07. |
| AUD-24 | Medium | Two tracker locations could diverge and copied phase links could break. | One tracker beside the specification; SPEC_ROOT and repo-root paths defined; all prompts synchronized. Relative-link validation covers all delivered MD. See README, START-HERE, PHASES, PROGRESS and every phase. |
| AUD-25 | High | Spawn/crash and cancel/commit boundaries could release locks with ambiguous live processes or contradictory ready/cancelled states. | Persist launch intent/PID identity, reconcile ambiguous launches conservatively, hold global permit through cleanup; commit reports its actual outcome. Test interrupted spawn/install/repair. See 02, 04 and P09. |
| AUD-26 | Medium | XUID fallback compatibility was required at G0 before the real-game phase, introducing another hidden ordering problem. | P04 records the authenticated source or explicit candidate; P09/P13 validate missing-XUID handling against actual games/server auth. No fabricated identifier. See 05. |
| AUD-27 | Medium | Archive, metadata and destination limits lacked some concrete enforcement and required internal-link behavior. | Bounded JSON/response/redirect/extraction policy, compatible internal links, post-DNS public-address checks and preserved OAuth-loopback separation. Validate approved runtime packages and unsafe fixtures. See 04 and 07. |

## File coverage

Every file below was reviewed and revised. References use the delivered layout.

| File | Main reviewed areas |
| --- | --- |
| [README.md](README.md) | Version, reading order, extraction layout, scope and audit entry points |
| [START-HERE.md](START-HERE.md) | Requirement precedence, agent instructions and tracker location |
| [PHASES.md](PHASES.md) | Dependency graph, blocked-auth policy and continuation prompts |
| [PROGRESS.md](PROGRESS.md) | Single source of progress and deferred real-account evidence |
| [01-PRODUCT-AND-SCOPE.md](01-PRODUCT-AND-SCOPE.md) | Immutable releases, platform claims and qualification build policy |
| [02-ARCHITECTURE-AND-STORAGE.md](02-ARCHITECTURE-AND-STORAGE.md) | JSON ownership, locks, receipts, storage and recovery |
| [03-AUTHENTICATION.md](03-AUTHENTICATION.md) | OAuth, request semantics, access decision, secrets and concurrency |
| [04-INSTALLATION-AND-SOURCES.md](04-INSTALLATION-AND-SOURCES.md) | Plan layers, sources, extraction, runtime integrity and repair |
| [05-LAUNCH-AND-LIFECYCLE.md](05-LAUNCH-AND-LIFECYCLE.md) | Arguments, environment, process supervision and window return |
| [06-UI-AND-IPC.md](06-UI-AND-IPC.md) | DTOs, snapshots, idempotency, errors and settings |
| [07-PACKAGING-AND-SECURITY.md](07-PACKAGING-AND-SECURITY.md) | OS dependencies, package identity, release and trust boundaries |
| [08-IMPLEMENTATION-AND-ACCEPTANCE.md](08-IMPLEMENTATION-AND-ACCEPTANCE.md) | Gates, test matrix, acceptance and evidence ownership |
| [09-SOURCES-AND-VERIFICATION.md](09-SOURCES-AND-VERIFICATION.md) | Dated metadata, source provenance and audit references |
| [P01](phases/P01-PROJECT-FOUNDATION.md) | Setup and tracker placement |
| [P02](phases/P02-CORE-STORAGE-AND-IPC.md) | Draft models, locks, epoch and request IDs |
| [P03](phases/P03-LIFECYCLE-SPIKE.md) | Early lifecycle spike and native-Quit cases |
| [P04](phases/P04-AUTHENTICATION-AND-ACCESS.md) | Positive-path completion versus final live matrix |
| [P05](phases/P05-CATALOG-AND-INSTALL-PLAN.md) | GamePlan and unresolved runtime totals |
| [P06](phases/P06-DOWNLOAD-ENGINE.md) | Persistent terminal state and event semantics |
| [P07](phases/P07-MANAGED-JAVA.md) | Extracted-tree integrity and execution ordering |
| [P08](phases/P08-COMPLETE-GAME-INSTALLER.md) | FinalInstallPlan, shared mutation and repair |
| [P09](phases/P09-REAL-GAME-LAUNCH.md) | Auth prerequisite, arguments, environment and recovery |
| [P10](phases/P10-BACKGROUND-AND-RETURN.md) | Quit modes, races, orphan and return behavior |
| [P11](phases/P11-COMPLETE-PRODUCT-UI.md) | Settings, busy/error recovery and real UI integration |
| [P12](phases/P12-CROSS-PLATFORM-PACKAGING.md) | Qualification package status and platform evidence |
| [P13](phases/P13-FINAL-QUALIFICATION.md) | Full live matrix and final artifact tests |
| [P14](phases/P14-HANDOVER.md) | Single tracker, final checksums and honest handover |

## Validation of this deliverable

The packaging check validates the final file count, UTF-8 readability, fenced-code balance, relative Markdown file links, P01–P14 coverage, phase dependency acyclicity, preservation of FR01–FR16/A01–A26 IDs and all initial not_started statuses. It compares the original archive with the reviewed source files, checks that every original file is retained and revised, and tests the final ZIP CRC/readback. Output contains only the 29 Markdown documents under one versioned directory; no game binaries, runtime packages, credentials or research scratch files are included.

The retained raw descriptors for 1.21.1 and 26.3 were rehashed against the original recorded descriptor hashes. This checks internal evidence consistency; it is not a fresh compatibility certification or a claim that historical metadata will never change.

## Remaining implementation gates

- Project-owned Microsoft registration, exact scopes/redirect and accepted Minecraft API contract still require real testing. Source inspection does not establish service approval.
- Owned, no-access, missing-profile and active-subscription behavior requires owner-operated account evidence under A03–A06. No credentials were used during this audit.
- Both Minecraft versions, managed Java packages, native libraries, real game/save/server authentication and GUI return require actual target-machine testing.
- Windows argfile locale behavior, native Tauri exit handling, macOS signing/notarization, Linux WebKitGTK/AppImage dependencies and runtime-tree repair remain implementation tests.
- Official metadata and external service contracts can change; dated source observations must be revalidated when coding. This revision does not replace the acceptance gates with document checks.

Use [CHANGELOG.md](CHANGELOG.md) for the concise change list and [START-HERE.md](START-HERE.md) to begin or resume development.
