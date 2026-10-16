# P13 — Full compatibility, security and performance qualification

**Goal:** Prove the complete MVP against both baseline releases and all claimed target platforms.

**Required inputs:** [P12](P12-CROSS-PLATFORM-PACKAGING.md)

**Traceability:** Completes G5 only with real evidence; all FR01–FR16 and A01–A26; especially A05, A12 and A23.

## Read before implementation

- [03-AUTHENTICATION.md](../03-AUTHENTICATION.md)
- [04-INSTALLATION-AND-SOURCES.md](../04-INSTALLATION-AND-SOURCES.md)
- [05-LAUNCH-AND-LIFECYCLE.md](../05-LAUNCH-AND-LIFECYCLE.md)
- [07-PACKAGING-AND-SECURITY.md](../07-PACKAGING-AND-SECURITY.md)
- [08-IMPLEMENTATION-AND-ACCEPTANCE.md](../08-IMPLEMENTATION-AND-ACCEPTANCE.md)

## Tasks

1. Complete the document 08 real-machine matrix, including 1.21.1/Java 21, 26.3/Java 25, Intel/Apple Silicon, Linux X11 and Wayland.
2. Run all remaining acceptance checks, including live owned/no-access/Game Pass states; report unavailable accounts or hardware explicitly.
3. Test unsafe paths/archives, credential isolation, production permissions, redacted export and absence of production mock bypasses.
4. Inject relevant failures: network outage, token expiry, disk full, permissions, corrupt files and abrupt process exit; fix concrete failures.
5. Measure cold/restored UI timing, idle/background CPU and complete launcher-process-tree memory; run 20 lifecycle cycles and check resource growth.

## Deliverables

- Filled docs/VERIFICATION.md and A01–A26 checklist with evidence.
- Corrected release candidates and honest support/known-limitation matrix.

## Done when

- [ ] Both qualified versions complete actual play/save/exit on every advertised target.
- [ ] Required acceptance checks pass; blocked checks remain explicitly blocked and prevent a full-MVP claim.
- [ ] No token leakage, production auth bypass, unsafe extraction or accumulating supervisor/window resources is found.
- [ ] Performance results identify reference hardware and measurement method instead of presenting assumed memory numbers.

## Demonstration

Show the completed matrix, demonstrate a representative failure/recovery path and reproduce one real game smoke test.

## Required evidence

Link every acceptance item to an actual test/result. Record dates, OS/hardware, app commit and runtime build.

## Keep out of this phase

Do not silently remove platforms, skip the second baseline, or relabel fixture success as real account/game verification.

## Prompt for the development chatbot

```text
Implement P13 — Full compatibility, security and performance qualification.
Read START-HERE.md, PHASES.md, phases/P13-FINAL-QUALIFICATION.md and its referenced specification sections.
Inspect current code and the single tracker at SPEC_ROOT/PROGRESS.md. Reuse completed work. Implement this phase, not a plan-only response.
Run the meaningful checks in Done when and record exact evidence. Do not replace real integrations with production mocks.
If an external dependency blocks completion, finish independent work in this phase, record the blocker, and do not claim completion.
Update progress and report changed components, checks, demo instructions, remaining blockers and the next eligible phase.
When this request names only this phase, stop after reporting its result.
```

## Final artifact gate

Complete live A03–A06 evidence carried from P04, then qualify both descriptor hashes on the required targets. Update the compatibility policy, rebuild final artifacts using the same locked code/dependencies, and run focused install/login/play/return smoke tests of these exact binaries. Record candidate and final checksums and explain their differences. Include extracted-runtime repair, cross-instance shared-file exclusion, JVM environment isolation, IPC epoch/request races and vault-failure recovery in regression evidence.

## Version 1.2.1 checks

Record MV01–MV12 outcomes and actual alternating-version cache reuse, JRE/JDK coexistence fixtures, interrupted commits, background install return and full-log-disk behavior. See [document 10](../10-MULTIVERSION-AND-CACHE.md).
