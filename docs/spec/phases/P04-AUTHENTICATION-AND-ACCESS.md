# P04 — Microsoft login and current game access

**Goal:** Complete real authentication with the project’s own registration and enforce an unambiguous Java play-access decision.

**Required inputs:** [P02](P02-CORE-STORAGE-AND-IPC.md)

**Traceability:** Completes the positive-auth portion of G0; full live account matrix belongs to P13/G5; FR01–FR03, FR15; A03–A08.

## Read before implementation

- [03-AUTHENTICATION.md](../03-AUTHENTICATION.md)
- [06-UI-AND-IPC.md](../06-UI-AND-IPC.md)
- [09-SOURCES-AND-VERIFICATION.md](../09-SOURCES-AND-VERIFICATION.md)

## Tasks

1. Verify the owner-supplied public client ID, registered loopback redirect, minimal scopes and Minecraft service access. Record the exact accepted contract.
2. Implement system-browser OAuth with S256 PKCE, state validation, expiring one-use callbacks, cancellation and refresh-token rotation.
3. Implement the Xbox user/XSTS and current Minecraft exchange, entitlement and authenticated profile adapters. Do not copy an obsolete endpoint recipe blindly.
4. Implement OS-vault storage and an explicit session-only fallback when the vault is unavailable; implement local sign-out.
5. Distinguish no access, subscription access, missing profile, app restriction, expired login and service failure. Add minimal account UI and deterministic negative tests.
6. Perform owner-operated real licensed/no-access/subscription tests where accounts are available. Keep a missing real positive login as a P04 blocker. Missing real owned/no-access/subscription matrix cases are P13/G5 blockers, not P04 prerequisites; fixtures never unlock production launch.

## Deliverables

- Real auth adapter, credential storage and minimal account flow.
- docs/AUTH-CONTRACT.md with non-secret endpoint/scopes/identity decisions.
- Verified-access model consumed only by backend launch preflight.

## Done when

- [ ] A real licensed account reaches verified_access with this project’s client ID.
- [ ] Deterministic policy tests deny no-access and accept the contract-defined subscription play entitlement without requiring ownership; missing real matrix cases are explicitly tracked for P13.
- [ ] PKCE/state failure, missing profile and service outage are correctly classified.
- [ ] Tokens never appear in React storage or exported logs; sign-out removes saved credentials.

## Demonstration

Sign in in the browser, display the authenticated Java name/access state, then sign out and verify launch authorization is cleared.

## Required evidence

Record synthetic tests separately from live account tests. Record provider error codes without credential payloads.

## Keep out of this phase

No copied client ID, offline/cracked/demo bypass, account creation or subscription purchase by the agent.

## Prompt for the development chatbot

```text
Implement P04 — Microsoft login and current game access.
Read START-HERE.md, PHASES.md, phases/P04-AUTHENTICATION-AND-ACCESS.md and its referenced specification sections.
Inspect current code and the single tracker at SPEC_ROOT/PROGRESS.md. Reuse completed work. Implement this phase, not a plan-only response.
Run the meaningful checks in Done when and record exact evidence. Do not replace real integrations with production mocks.
If an external dependency blocks completion, finish independent work in this phase, record the blocker, and do not claim completion.
Update progress and report changed components, checks, demo instructions, remaining blockers and the next eligible phase.
When this request names only this phase, stop after reporting its result.
```

## Additional regression checks

Test late auth callbacks after cancel/sign-out, serialized refresh rotation, unknown 403 errors, external-browser timeout and vault-deletion failure across restart. Verify the actual registered redirect host/path/port and accepted Xbox scope profile with the owner’s public client. Do not log payloads from the reference implementation.
