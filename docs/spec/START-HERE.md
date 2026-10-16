# Instructions to the implementation agent

You are implementing the desktop launcher described in this specification. Produce a functioning application, not just a design, simulated download screen, or mock authentication demo.

## Working contract

1. Read all files listed in README.md before changing architecture. The numbered documents are normative; examples are illustrative unless explicitly described as a contract.
2. Inspect the existing project first. Preserve user work. If there is no project, create the Tauri 2 + React + TypeScript + Vite project with a Rust core.
3. Execute the P01–P14 task files linked from PHASES.md. They are the authoritative work sequence; document 08 defines G0–G5 quality gates and A01–A26 acceptance checks. Track implementation only in `SPEC_ROOT/PROGRESS.md`, beside PHASES.md; the recommended SPEC_ROOT is repository `docs/spec/`. Prompt file names resolve there, while commands run at the repository root. Read each phase’s prerequisites, deliverables and Done when checklist.
4. Use current compatible stable dependencies and commit the JS and Cargo lockfiles. Record toolchain versions and native OS dependencies. Do not assume a version number from this specification is a package version to install.
5. Build a minimal vertical slice as soon as possible: browser login, verified access, install one baseline, actual Java launch, exit, restored UI.
6. Never replace real auth with a hard-coded nickname, random UUID, copied launcher client ID, public profile lookup, or a permanent local `licensed=true` flag.
7. Keep secrets in Rust and an OS credential vault. Test credentials and authenticated payloads do not belong in repository files or chat transcripts.
8. All unimplemented core paths must return an explicit unsupported/error result. A disabled feature must not display fake success. Mock adapters may exist only in test targets and a clearly separated UI development harness that cannot launch a real game.
9. If Microsoft/Minecraft access is blocked externally, implement the remaining authorized modules and tests using deterministic fixtures. Mark the real-login gate blocked with the actual non-secret reason. Do not advertise a completed MVP.
10. Do not assume every undocumented service endpoint still works. Resolve the auth contract against current upstream source and the project's approved client ID, then freeze that contract with tests and a short decision record.
11. Prefer cohesive Rust modules and a few typed interfaces. No microservices, Docker requirement for end users, database server, Electron, Node sidecar, or separate watchdog executable for MVP.
12. Record any deviation, reason, user impact and test evidence in `docs/DECISIONS.md`. Do not silently shrink the acceptance matrix.

## Expected implementation deliverables

- Full source repository, lockfiles, build scripts and configuration templates with no embedded secrets.
- All MVP screens connected to real Rust operations.
- Deterministic tests for resolver, integrity, extraction, auth decisions, command construction and lifecycle.
- CI for supported build targets; package artifacts for locally/CI-tested targets.
- Developer setup guide, Microsoft application setup guide, user installation guide and troubleshooting guide.
- `docs/VERIFICATION.md` listing actual pass/fail/blocked results, OS/architecture, game version, runtime build and commit.
- License notices for distributed dependencies, no bundled Minecraft game binaries or borrowed branding.

## Configuration owned by the project owner

| Setting | Development treatment | Release treatment |
| --- | --- | --- |
| Product name / application identifier | Use a clearly provisional name and identifier | Owner chooses a stable unique identity before first release |
| Microsoft client ID | Read from a documented non-secret build configuration | Must belong to this project and pass the service-access gate |
| Redirect URI and OAuth scopes | Validate against the actual public-client registration | Freeze the verified values in a documented auth profile |
| Windows/macOS signing identity | Unsigned local builds may be tested | Configure CI secrets; never commit private keys |
| Download/release page | A local build may omit update navigation | Provide an HTTPS destination if manual update links are shown |

Missing branding is not a reason to stop implementation. Missing access approval is not permission to impersonate another application.

## Completion report

Lead with what actually works. Include exact commands to run the app and tests, artifact locations, checked acceptance IDs, and external blockers. Distinguish static validation, mocked integration tests, real API tests and real game launches. Do not say “cross-platform tested” if only one OS was used.

## Phase-by-phase requests

If asked to implement one named phase, complete that phase and report its evidence before ending the turn. If asked to implement the whole MVP, continue through eligible phases without routine confirmation requests. Follow PHASES.md for external blockers and report partial progress honestly. Functional requirement IDs are FR01–FR16; P identifiers always refer to implementation phases.

## Second-review requirements

Read document 10 before implementing storage/resolution or resuming code written from version 1.2.0. Cache-path changes in 1.2.1 supersede the earlier layout. If code/data already exists, preserve it and implement a verified lazy import into canonical paths; do not wipe DATA_ROOT. No application data migration was executed while writing this specification. Read SECOND-AUDIT.md for the bounded set of second-review corrections.
