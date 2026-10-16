# P01 — Project foundation and external prerequisites

**Goal:** Create a reproducible desktop project and identify external authentication requirements before substantial feature work.

**Required inputs:** None; this is the starting phase.

**Traceability:** Starts G0; contributes to G1 and A25.

## Read before implementation

- [START-HERE.md](../START-HERE.md)
- [01-PRODUCT-AND-SCOPE.md](../01-PRODUCT-AND-SCOPE.md)
- [02-ARCHITECTURE-AND-STORAGE.md](../02-ARCHITECTURE-AND-STORAGE.md)
- [03-AUTHENTICATION.md](../03-AUTHENTICATION.md)
- [07-PACKAGING-AND-SECURITY.md](../07-PACKAGING-AND-SECURITY.md)

## Tasks

1. Inspect the repository and preserve existing work. Initialize Tauri 2, React, TypeScript and Vite if needed. Keep production frontend assets local.
2. Pin compatible stable Rust/Node/package versions, commit both lockfiles, and implement real dev, typecheck, lint, test and build commands.
3. Create the module directories from document 02, a basic accessible app window, and CI for the core checks.
4. Write docs/DECISIONS.md and docs/AUTH-SETUP.md: provisional app identity, target platforms, own public-client ID, redirect/scopes to verify, and service-access approval requirements. The owner supplies/authorizes external account setup; never invent credentials.
5. Record available and missing OS test environments. Keep the extracted specification at docs/spec/ and its single PROGRESS.md tracker current; record exact target OS versions before qualification.

## Deliverables

- A runnable desktop shell with a real React view.
- Committed dependency locks and verified project scripts.
- A concise external-prerequisite list with owner actions, not fabricated approval.

## Done when

- [ ] The app opens locally using the documented command.
- [ ] Frontend checks and Rust compilation pass.
- [ ] A release build of the shell succeeds on the available host.
- [ ] Missing Microsoft approval is tracked but does not block creating the shell.

## Demonstration

Run the app, change a React label/style, and demonstrate the development feedback loop.

## Required evidence

Record commands, tool versions, host OS and CI results; identify external credentials or platforms not yet available.

## Keep out of this phase

Do not build the installer engine, game launch, final visual design or a product backend service.

## Prompt for the development chatbot

```text
Implement P01 — Project foundation and external prerequisites.
Read START-HERE.md, PHASES.md, phases/P01-PROJECT-FOUNDATION.md and its referenced specification sections.
Inspect current code and the single tracker at SPEC_ROOT/PROGRESS.md. Reuse completed work. Implement this phase, not a plan-only response.
Run the meaningful checks in Done when and record exact evidence. Do not replace real integrations with production mocks.
If an external dependency blocks completion, finish independent work in this phase, record the blocker, and do not claim completion.
Update progress and report changed components, checks, demo instructions, remaining blockers and the next eligible phase.
When this request names only this phase, stop after reporting its result.
```
