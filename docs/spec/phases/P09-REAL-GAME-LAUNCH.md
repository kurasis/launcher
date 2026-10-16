# P09 — Real authenticated Minecraft launch

**Goal:** Launch the first baseline with valid real-session credentials and supervise it correctly while keeping the GUI open.

**Required inputs:** [P04](P04-AUTHENTICATION-AND-ACCESS.md), [P08](P08-COMPLETE-GAME-INSTALLER.md)

**Traceability:** G3 real-launch component; FR10; A12–A13, A16, A20 and launch enforcement.

## Read before implementation

- [03-AUTHENTICATION.md](../03-AUTHENTICATION.md)
- [05-LAUNCH-AND-LIFECYCLE.md](../05-LAUNCH-AND-LIFECYCLE.md)
- [06-UI-AND-IPC.md](../06-UI-AND-IPC.md)

## Tasks

1. Implement backend launch preflight: locks, installed-file/runtime checks and fresh authenticated access/profile verification.
2. Compile ordered JVM/game arguments from metadata with complete placeholders, memory settings, logging and platform-specific native paths.
3. Implement tested Java argfile escaping for long Windows classpaths and secret-aware cleanup.
4. Spawn the exact managed Java executable without a shell; capture both output streams concurrently, redact secrets and bound logs.
5. Track spawn/exit/crash and explicit Stop behavior. Run 1.21.1 with a real licensed account and validate a local world plus an authorized online-mode server.

## Deliverables

- Production launch compiler and ProcessSupervisor.
- First actual licensed play/exit vertical slice with usable diagnostics.

## Done when

- [ ] The real game reaches its menu and can create, save and reload a test world.
- [ ] An authorized online-mode server accepts the real game session.
- [ ] Duplicate launch is rejected; no-access or unverifiable access never spawns Java.
- [ ] Spaces/Unicode/long arguments work and logs remain bounded/redacted.

## Demonstration

Sign in, Play the installed baseline, enter a test world, exit normally and show the recorded result.

## Required evidence

Record actual version, runtime, OS, launch result and server-auth test separately from command-builder unit tests.

## Keep out of this phase

Real launch is blocked without verified own-client authentication. Do not substitute a fake session. GUI unloading comes in P10.

## Prompt for the development chatbot

```text
Implement P09 — Real authenticated Minecraft launch.
Read START-HERE.md, PHASES.md, phases/P09-REAL-GAME-LAUNCH.md and its referenced specification sections.
Inspect current code and the single tracker at SPEC_ROOT/PROGRESS.md. Reuse completed work. Implement this phase, not a plan-only response.
Run the meaningful checks in Done when and record exact evidence. Do not replace real integrations with production mocks.
If an external dependency blocks completion, finish independent work in this phase, record the blocker, and do not claim completion.
Update progress and report changed components, checks, demo instructions, remaining blockers and the next eligible phase.
When this request names only this phase, stop after reporting its result.
```

## Additional regression checks

Verify JVM environment-variable filtering for both probes and game launch; test Java 21/25 argument-file encoding under non-ASCII Windows user paths/locales. Trigger Repair-needed during preflight and ensure it releases the global permit without recursive-lock deadlock. Test idempotent repeated launch request IDs and conservative recovery of an interrupted spawn.

## Version 1.2.1 checks

Validate classpath-compatible DATA_ROOT before large downloads and again before launch. Serialize cancel versus spawn and keep output drains alive after a disk-full log-sink failure. See [document 10](../10-MULTIVERSION-AND-CACHE.md).
