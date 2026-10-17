# Verification record

Actual results only. Each entry names the host, the commit or pull request and the kind of
evidence: **static** (lint/type/compile), **automated** (tests with fixtures or mock
runtime), **host GUI** (app run on the development host), **CI** (GitHub-hosted runners),
**live API** or **real game**. Nothing here is a target-machine qualification unless it
says so; the qualification matrix is completed in P13.

From P04 on, a phase's **CI results** are recorded by the following phase's pull request,
because adding them to the same pull request would start another full CI run. The phase
is merged only after its own required checks pass.

## Test environments

| Environment                                            | Available   | Used for                     | Notes                                                                                                |
| ------------------------------------------------------ | ----------- | ---------------------------- | ---------------------------------------------------------------------------------------------------- |
| Ubuntu 24.04.5 LTS x86_64 container (development host) | Yes         | Build, tests, GUI under Xvfb | No GPU, no window manager, no desktop session, no Secret Service vault. Not a qualification desktop. |
| GitHub-hosted `ubuntu-24.04`                           | Yes (CI)    | Rust checks + release build  | Compile/test only, no GUI run                                                                        |
| GitHub-hosted `windows-2025` (x64)                     | Yes (CI)    | Rust checks + release build  | Compile/test only, no GUI run                                                                        |
| GitHub-hosted `macos-15` (arm64)                       | Yes (CI)    | Rust checks + release build  | Compile/test only, no GUI run                                                                        |
| GitHub-hosted `macos-15-intel` (x64)                   | Yes (CI)    | Rust checks + release build  | Compile/test only, no GUI run                                                                        |
| Windows 11 x64 desktop                                 | **Missing** | Qualification (P12/P13)      | Owner to provide; record exact build                                                                 |
| macOS Apple Silicon desktop                            | **Missing** | Qualification                | Owner to provide; record macOS version                                                               |
| macOS Intel desktop                                    | **Missing** | Qualification                | Owner to provide; record macOS version                                                               |
| Ubuntu 24.04 desktop, X11 and Wayland sessions         | **Missing** | Qualification                | Owner to provide                                                                                     |
| Debian 12 desktop                                      | **Missing** | Qualification                | Owner to provide                                                                                     |

Exact target OS versions for qualification are recorded here before P13 starts.

## P01 — Project foundation

- **Host:** Ubuntu 24.04.5 LTS, x86_64, 4 vCPU, container without GPU. GUI checks ran under
  Xvfb (`1280x800x24`) with `dbus-run-session`.
- **Tools:** Node 24.21.0, npm 11.19.0, rustc/cargo 1.97.0, Tauri 2.12.4 (CLI 2.12.3, wry
  0.57.0, tao 0.37.1), WebKitGTK 4.1 2.52.6.
- **Branch / PR:** `claude/exciting-cori-pldz43`, [kurasis/launcher#1](https://github.com/kurasis/launcher/pull/1).

| Check                            | Command                                                                   | Evidence kind | Result                                                                                                                                                                                                                                                                                       |
| -------------------------------- | ------------------------------------------------------------------------- | ------------- | -------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------- |
| Dependency install from lockfile | `npm ci`                                                                  | static        | Pass, 0 vulnerabilities reported, no engine warnings                                                                                                                                                                                                                                         |
| TypeScript                       | `npm run typecheck`                                                       | static        | Pass                                                                                                                                                                                                                                                                                         |
| ESLint                           | `npm run lint`                                                            | static        | Pass, 0 warnings                                                                                                                                                                                                                                                                             |
| Prettier                         | `npm run format:check`                                                    | static        | Pass                                                                                                                                                                                                                                                                                         |
| Frontend tests                   | `npm test`                                                                | automated     | Pass, 8 tests in 2 files                                                                                                                                                                                                                                                                     |
| Frontend production build        | `npm run build`                                                           | static        | Pass                                                                                                                                                                                                                                                                                         |
| rustfmt                          | `npm run rust:fmt`                                                        | static        | Pass                                                                                                                                                                                                                                                                                         |
| Clippy                           | `npm run rust:lint`                                                       | static        | Pass, `-D warnings`                                                                                                                                                                                                                                                                          |
| Rust tests                       | `npm run rust:test`                                                       | automated     | Pass, 13 tests (6 unit; 2 IPC via the Tauri mock runtime, including remote-origin denial; 5 security configuration)                                                                                                                                                                          |
| Release build                    | `npx tauri build --bundles deb`                                           | static        | Pass on host (3 min 57 s); binary `src-tauri/target/release/kurasis-launcher`, `.deb` 1.77 MiB declaring `libwebkit2gtk-4.1-0, libgtk-3-0`                                                                                                                                                   |
| Release app opens                | Run release binary under Xvfb                                             | host GUI      | Pass: 1100×720 window "Kurasis Launcher"; React view renders under the production CSP and shows data from Rust `get_app_info` (Release, linux / x86_64, Tauri 2.12.4, Microsoft client ID "Not configured"). Screenshot: [p01-release-linux-x64.png](verification/p01-release-linux-x64.png) |
| Development loop                 | `npm run tauri dev`, then edit `src/lib/strings.ts` and `src/app/App.tsx` | host GUI      | Pass: app opened in debug mode ([before](verification/p01-dev-before.png)); changing the build-tag label and badge tone hot-reloaded without restart ([after](verification/p01-dev-hmr-after.png)); change reverted                                                                          |
| CI                               | `.github/workflows/ci.yml`                                                | CI            | See the CI section below                                                                                                                                                                                                                                                                     |

### CI results

Workflow run [38977807206](https://github.com/kurasis/launcher/actions/runs/38977807206) on
commit `6fbcbb0` (PR #1). Earlier runs on the same PR failed on Windows and were fixed
in the PR: the integration test executable lacked the Common Controls v6 manifest
(`STATUS_ENTRYPOINT_NOT_FOUND`, see D-011), and the IPC test used a non-Windows origin.

| Job                                                                   | Result |
| --------------------------------------------------------------------- | ------ |
| Frontend checks (ubuntu-24.04)                                        | Pass   |
| Rust core linux-x64 (ubuntu-24.04): fmt, clippy, tests, release build | Pass   |
| Rust core windows-x64 (windows-2025): clippy, tests, release build    | Pass   |
| Rust core macos-arm64 (macos-15): clippy, tests, release build        | Pass   |
| Rust core macos-x64 (macos-15-intel): clippy, tests, release build    | Pass   |

### Not verified in P01

- No GUI run on Windows or macOS; CI only compiles and tests there.
- No Wayland session, no real desktop environment, no screen reader run.
- No Microsoft, Xbox or Minecraft service call (no project client ID exists yet).
- Installers are not validated (P12). The `.deb` was built only to confirm the bundler runs.

## P02 — Rust state, storage and typed IPC

- **Host:** same development host as P01 (Ubuntu 24.04.5 x86_64 container, Xvfb).
- **Branch / PR:** `claude/p02-core-storage-ipc`, [kurasis/launcher#2](https://github.com/kurasis/launcher/pull/2).

| Check                                    | Command / scenario                                                                                                                                                    | Evidence kind        | Result                                                                                                                                                                                                                                     |
| ---------------------------------------- | --------------------------------------------------------------------------------------------------------------------------------------------------------------------- | -------------------- | ------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------ |
| Rust unit tests                          | `npm run rust:test`                                                                                                                                                   | automated            | Pass: 80 unit tests (storage, documents, locks, reconciliation, paths, validation, errors, redaction, activity permit, request ledger, state)                                                                                              |
| Rust integration tests                   | `npm run rust:test`                                                                                                                                                   | automated            | Pass: 9 (2 IPC via mock runtime, 2 cross-process DATA_ROOT lock, 5 security configuration)                                                                                                                                                 |
| Frontend tests                           | `npm test`                                                                                                                                                            | automated            | Pass: 19 (store reconciliation, single listener registration and cleanup, MutationRequest wrapping, UI flows)                                                                                                                              |
| Typecheck, lint, format, clippy, rustfmt | `npm run check`                                                                                                                                                       | static               | Pass                                                                                                                                                                                                                                       |
| Create, change memory, restart           | Debug build with `KURASIS_DATA_ROOT` set to a path with spaces and Cyrillic characters; create "Survival world" (1.21.1), set 6144 MiB, kill the process, start again | host GUI             | Pass: installation, selection and 6144 MiB restored ([created](verification/p02-instance-created.png), [after restart](verification/p02-restored-after-restart.png)); `instance.json` has `manifestSha1: null` and `health: not_installed` |
| Second process on the same DATA_ROOT     | Start a second launcher process while the first runs                                                                                                                  | host GUI + automated | Pass: second process shows `DATA_ROOT_LOCKED` and writes nothing ([screenshot](verification/p02-second-process-locked.png)); `tests/data_root_lock.rs` proves the same across processes                                                    |

### P02 "Done when" mapping

| Criterion                                                                       | Evidence                                                                                                                                                                                                                                                 |
| ------------------------------------------------------------------------------- | -------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------- |
| Create an instance and restart: state and selection survive                     | Demo above; `state::tests::created_instance_and_selection_survive_restart`, `storage::tests::instances_survive_reopen`                                                                                                                                   |
| Malformed inputs and unknown schemas handled without data loss                  | `state::tests::invalid_inputs_are_rejected_without_changes`, `malformed_payloads_are_rejected_by_deserialization`, `unsupported_settings_schema_blocks_mutation_without_data_loss`, `document::tests::newer_schema_is_left_untouched`, `corrupt_*` tests |
| Interrupted writes preserve a readable prior version                            | `atomic::tests::interrupted_write_keeps_previous_version_and_is_cleaned_up`, `storage::tests::interrupted_instance_write_leaves_readable_previous_version`                                                                                               |
| Invalid IDs cannot escape DATA_ROOT; repeated subscriptions cleaned up          | `paths::tests::instance_ids_only_accept_canonical_uuids`, `instance_paths_stay_inside_root`, `safe_names_reject_traversal_and_reserved_names`; `ipc.test.ts` "registers one listener per event…"                                                         |
| 1.2.1: DATA_ROOT lifetime lock, two contending processes                        | `tests/data_root_lock.rs`, `state::tests::locked_data_root_makes_state_unavailable`, demo above                                                                                                                                                          |
| 1.2.1: canonical cache keys, separate InstallationHealth, summary/lock mismatch | `paths::tests::canonical_content_paths_use_digests_not_names`, `runtime_key_separates_image_type_and_archive`, `reconcile::tests::*`, `storage::tests::stale_summary_is_reconciled_and_rewritten`                                                        |
| No token fields in DTOs                                                         | `state::tests::snapshot_exposes_no_secret_fields`, `models::tests::account_document_has_no_secret_fields`, `secrets::tests::*`                                                                                                                           |

### CI results

Workflow run [38981697137](https://github.com/kurasis/launcher/actions/runs/38981697137) on
commit `f919cc0` (PR #2): frontend checks, and clippy, tests and release shell build on
linux-x64 (ubuntu-24.04), windows-x64 (windows-2025), macos-arm64 (macos-15) and macos-x64
(macos-15-intel) — all 5 jobs pass. The cross-process lock test and the atomic replacement
tests therefore also ran on Windows and both macOS architectures.

## P03 — Early window and process lifecycle proof

- **Host:** development host (Ubuntu 24.04.5 x86_64 container) with Xvfb, the openbox
  window manager (so window close requests are real `WM_DELETE_WINDOW` messages) and a
  shared D-Bus session bus for the single-instance plugin. Debug build with embedded assets
  (`npx tauri build --debug --no-bundle`), `KURASIS_DATA_ROOT` pointing to a scratch
  directory. Window, supervisor and child counts come from `xdotool` and `pgrep`.
- **Branch / PR:** `claude/p03-lifecycle-spike`, [kurasis/launcher#3](https://github.com/kurasis/launcher/pull/3).

| Scenario                                                | Expected                                                                                             | Observed                                                                                                                                                                         | Result                      |
| ------------------------------------------------------- | ---------------------------------------------------------------------------------------------------- | -------------------------------------------------------------------------------------------------------------------------------------------------------------------------------- | --------------------------- |
| Run 12 s, default mode                                  | Window destroyed after ~1.5 s; supervisor and child keep running; one window returns with the result | 1 → 0 windows during the run, supervisor 1, child 1; after exit 1 window, "Exited normally", exit code 0, 12.3 s ([screenshot](verification/p03-restored-after-exit.png))        | Pass                        |
| Reopen the app while the child runs (window unloaded)   | Existing supervisor recreates exactly one window; no second supervisor                               | 0 → 1 window, supervisors 1, child still running; after exit still exactly 1 window ([screenshot](verification/p03-reopened-during-run.png))                                     | Pass                        |
| Child exits immediately                                 | Window never unloaded                                                                                | Minimum 1 window throughout                                                                                                                                                      | Pass                        |
| Child crashes with exit code 3                          | Window restored with an error result                                                                 | "Exited with an error", exit code 3 ([screenshot](verification/p03-crash-result.png))                                                                                            | Pass                        |
| Grandchild keeps output pipes open                      | Result after the 2 s drain bound; window not unloaded after the process ended                        | First attempt: window was unloaded during the drain (defect). Fixed by disarming the unload on process exit; retest: minimum 1 window, result after ~2.3 s with the drain notice | Pass after fix              |
| Heavy stdout/stderr (~32 MiB each)                      | No blocking, bounded tail                                                                            | Run finished in under 3 s; tail bounded                                                                                                                                          | Pass                        |
| `keep_open` mode, user closes the window during the run | Window stays during the run; WM close keeps supervisor and child alive; one window after exit        | 1 window during run; after `wmctrl -c` 0 windows, supervisor 1, child 1; after exit 1 window                                                                                     | Pass                        |
| Quit during a run                                       | Choice dialog (Keep in background default)                                                           | Dialog shown ([screenshot](verification/p03-quit-choice.png)); "Stop and quit" stopped the child and exited in 0.4 s; no processes left                                          | Pass                        |
| Window close while idle                                 | Normal exit                                                                                          | Process exited after `wmctrl -c`                                                                                                                                                 | Pass                        |
| Quit button while idle                                  | Normal exit                                                                                          | Process exited                                                                                                                                                                   | Pass                        |
| 20 unload/restore cycles (4 s child)                    | Exactly one window and one supervisor each cycle, no leftover children, stable process count         | 20/20 cycles pass; WebKit process count constant (4); supervisor RSS 180.5 → 200.0 MiB ([CSV](verification/p03-lifecycle-cycles.csv))                                            | Pass                        |
| 20 further cycles (21–40)                               | No failures; check whether memory keeps growing                                                      | 20/20 pass; RSS 199.9 → 201.6 MiB (≈ 85 KiB per cycle)                                                                                                                           | Pass; residual growth noted |

Memory note: most of the growth happened in the first cycles (WebKit/GTK warm-up); cycles
21–40 still grew by about 85 KiB per recreation in this debug build. This is recorded as a
watch item for A23, which must be measured on release builds on every target in P13.

| Automated check                                                                                                                                                                                                      | Result                                                                                                                                                                                                                                                                                                                                |
| -------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------- | ------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------- |
| `tests/child_supervision.rs`: exit code, process-exit hook before drain, 32 MiB simultaneous output, inherited-pipe drain ≤ 2 s + margin, Stop kills only the owned child while an identical bystander keeps running | Pass (5 tests)                                                                                                                                                                                                                                                                                                                        |
| Lifecycle unit tests: exit policy, unload token invalidation, quit intent                                                                                                                                            | Pass                                                                                                                                                                                                                                                                                                                                  |
| Harness: fixed scenarios, rejects arbitrary values; release handler list has no harness command                                                                                                                      | Pass                                                                                                                                                                                                                                                                                                                                  |
| Frontend: quit choice flows (keep, stop and quit, cancel, Escape, OS/menu quit event), run result rendering, game-state reconciliation                                                                               | Pass (27 frontend tests total)                                                                                                                                                                                                                                                                                                        |
| Release binary without harness (`npx tauri build --no-bundle`)                                                                                                                                                       | Pass: About page has no lifecycle panel ([screenshot](verification/p03-release-no-harness.png)); the child flag string is absent from the release binary and starting it with the flag runs the normal app; the release handler list has no harness command (the name only appears in the bundled frontend/ACL text, with no handler) |

### Not verified in P03

- Windows 11, macOS (both architectures) and Wayland window behaviour; macOS menu Quit,
  Cmd+Q and Dock reopen. CI compiles and runs the automated tests on Windows and macOS only.
- Real Minecraft (P09/P10). The test child is never offered in release builds.

### CI results

Workflow run [38985482313](https://github.com/kurasis/launcher/actions/runs/38985482313) on
commit `bb8e7a7` (PR #3): all 5 jobs pass (frontend; clippy, tests and release build on
linux-x64, windows-x64, macos-arm64, macos-x64). The first run on this PR failed on Windows:
`Supervised::kill` could report success after the child had exited because the supervisor
thread had not yet released its channel. Fixed by recording exit in a flag set right after
`wait()`; the test then passed 15 times in a row locally and on all CI runners. The child
supervision tests therefore ran on Windows and both macOS architectures.

## P04 — Microsoft login and current game access

- **Host:** development host (Ubuntu 24.04.5 x86_64 container). No Secret Service is running
  in this container, so the OS-vault test exercises the explicit "unavailable" path here.
- **Branch / PR:** `claude/p04-auth-access`, [kurasis/launcher#4](https://github.com/kurasis/launcher/pull/4).
- **Evidence kind:** synthetic only. All flows ran against a local fixture HTTP server. **No
  live Microsoft, Xbox or Minecraft request was made**: no project-owned client ID exists.

### Synthetic tests (pass)

| Area                       | Tests                                                                                                                                                                                                                                                                                                                                                                                             |
| -------------------------- | ------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------- |
| PKCE/state                 | RFC 7636 Appendix B vector; fresh 43-char verifiers; constant-time state comparison                                                                                                                                                                                                                                                                                                               |
| Loopback callback          | Only the matching state on `/` is accepted; wrong state, other path, duplicate parameter and garbage are answered 400/404 without consuming the attempt; the code is never echoed; provider `access_denied`; timeout; cancel; late callback after cancel reaches no listener; oversized request                                                                                                   |
| Protocol parsing           | Token, Xbox, XSTS, Minecraft token, entitlements (missing `items` = protocol error), profile; OAuth error classification without copying descriptions; request bodies match the contract; no client secret                                                                                                                                                                                        |
| Access decision            | Owned and subscription (no `product_minecraft`) accepted; no play access denied even with ownership item; profile not requested without play access                                                                                                                                                                                                                                               |
| End-to-end flows (fixture) | Successful login (PKCE verifier matches challenge, same redirect URI, Bearer only to Minecraft endpoints, refresh token only in the vault, no secret in `accounts.json` or the snapshot); no access ×2; malformed entitlements; profile 404; `Invalid app registration` 403 → app not approved; other 403 → forbidden; XSTS restricted and missing Xbox profile; 503; 429; XSTS identity mismatch |
| Concurrency                | Repeated begin returns the same attempt; wrong state does not consume the attempt; a late refresh finishing after sign-out cannot restore the account; sign-in/sign-out refused while a launch owns the account                                                                                                                                                                                   |
| Refresh                    | Restore refreshes and verifies; rotated refresh token is persisted; non-rotated keeps the old one; `invalid_grant` clears the saved login; outage keeps it                                                                                                                                                                                                                                        |
| Launch authorization       | Fresh entitlements for every launch, cached Minecraft token reused, one refresh on 401, revoked access caught on the next launch                                                                                                                                                                                                                                                                  |
| Sign-out                   | Removes vault entry, record and launch authorization; vault deletion failure → `SIGN_OUT_PENDING` marker, retried on restart before any restore                                                                                                                                                                                                                                                   |
| Session-only mode          | Vault write failure → in-memory credential, nothing persisted, UI notice                                                                                                                                                                                                                                                                                                                          |
| External links             | Allowlist accepts only fixed HTTPS hosts and the authorize path                                                                                                                                                                                                                                                                                                                                   |
| UI                         | Not-configured explanation without sign-in button or password field; sign-in, cancel, sign-out, check-again, help links; no-access vs outage presentation ([screenshot](verification/p04-account-not-configured.png))                                                                                                                                                                             |

### Live checks (blocked)

| Check                                                                                                   | Status        | Needed from the owner                                        |
| ------------------------------------------------------------------------------------------------------- | ------------- | ------------------------------------------------------------ |
| Real licensed account reaches `verified_access` with this project's client ID (P04 done criterion, A03) | **Blocked**   | Project-owned Microsoft client ID and Minecraft API approval |
| Real no-access account denied (A04)                                                                     | Blocked (P13) | Test account without Java access                             |
| Active Game Pass accepted (A05)                                                                         | Blocked (P13) | Test account with an active subscription                     |
| Profile-not-created and outage seen live (A06)                                                          | Blocked (P13) | As above                                                     |
| Accepted redirect form, scope spelling, `Invalid app registration` wording                              | Unverified    | First live sign-in                                           |

### CI results

Workflow run [38987094069](https://github.com/kurasis/launcher/actions/runs/38987094069) on
commit `be35772` (PR #4): all 5 jobs pass (frontend; clippy, tests and release build on
linux-x64, windows-x64, macos-arm64, macos-x64). The OS-vault code paths compiled and the
synthetic auth tests ran on all four targets; no live Microsoft request was made anywhere.

## P05 — Minecraft metadata and deterministic install planning

- **Host:** development host (Ubuntu 24.04.5 x86_64 container), outbound HTTPS through the
  environment's proxy.
- **Branch / PR:** `claude/p05-catalog-plan`, [kurasis/launcher#5](https://github.com/kurasis/launcher/pull/5).
- **Evidence kind:** metadata only. Planning proves the official metadata is read and
  resolved correctly; it says nothing about whether the game starts (that is P09/P13). No
  game binary was downloaded.

### Deterministic tests (pass)

| Area                     | Tests                                                                                                                                                                                                                                                                                                                                                                     |
| ------------------------ | ------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------- |
| Rules engine (9)         | No rules = allow; rules start disallowed; **last matching rule wins** (allow-then-disallow and disallow-then-allow); predicates are conjunctive; `os.version` regex; features default to false; unknown actions, OS keys, rule keys and non-boolean features are errors; an `os.version` rule with an unknown OS version is an error                                      |
| Planner (16)             | Fixtures are byte-identical to the catalog descriptors (SHA-1); library selection per target matches an independent reference (below); Java 21/25 from metadata; foreign-OS libraries excluded; ordered canonical classpath ending in the client; per-OS arguments; logging config and asset index planned; every URL on the allowlist; deterministic serializable output |
| Assets                   | Objects deduplicated by hash, names never used as paths; legacy `virtual`/`map_to_resources` indexes, bad hashes, missing sizes and conflicting sizes for one hash rejected                                                                                                                                                                                               |
| Rejection                | `inheritsFrom`, legacy `minecraftArguments`, non-release type, ID mismatch, missing client or `javaVersion`, unsafe main class, legacy `natives`/classifiers, unsafe library paths, off-allowlist or plain-HTTP URLs, invalid hashes, unknown placeholders; unsupported targets explicit                                                                                  |
| Content identity (1.2.1) | Same coordinate with different digests → distinct content; identical digest reused; conflicting size for one digest rejected; descriptors stored per version and SHA-1                                                                                                                                                                                                    |
| Catalog / cache (9)      | Entry parsing; ETag revalidation (304) and 6 h freshness; outage shows cached catalog as stale; no cache → "catalog unavailable", nothing invented; changed descriptor hash → not trusted and refused for planning; `latest.release` never adds a version; descriptors verified, stored byte-for-byte, reused, refetched when damaged; wrong bytes never stored           |
| Network policy (3)       | Exact hosts per source category; special-purpose IPv4/IPv6 ranges are not public                                                                                                                                                                                                                                                                                          |
| UI (3)                   | Availability badge, metadata-only estimate with runtime size "unknown", no estimate for a changed descriptor, stale notice and forced refresh                                                                                                                                                                                                                             |

### Independent cross-check

Library selection was recomputed by a separate Python script (not sharing code with the
Rust planner): last matching rule wins, no features, artifacts deduplicated by SHA-1.

```python
import json, re, sys

def allowed(rules, os_name, arch):
    if not rules:
        return True
    action = False
    for rule in rules:
        os_ = rule.get("os", {})
        if "features" in rule:
            continue
        if "name" in os_ and os_["name"] != os_name:
            continue
        if "arch" in os_ and not re.fullmatch(os_["arch"], arch):
            continue
        action = rule["action"] == "allow"
    return action

for path in sys.argv[1:]:
    desc = json.load(open(path))
    for os_name, arch in [("windows", "x86_64"), ("linux", "x86_64"), ("osx", "x86_64"), ("osx", "arm64")]:
        seen = {}
        for lib in desc["libraries"]:
            art = lib.get("downloads", {}).get("artifact")
            if art and allowed(lib.get("rules"), os_name, arch):
                seen[art["sha1"]] = art["size"]
        print(desc["id"], os_name, arch, len(seen), sum(seen.values()))
```

Its output equals the expectations in `library_selection_matches_the_independent_reference`
(e.g. 1.21.1 Windows 70 libraries / 66,739,500 bytes, Linux 56 / 61,944,833; 26.3 Windows 74 /
87,763,720, macOS 80 / 91,265,044) and the counts in document 10.

### Live metadata check

`cargo test --test live_metadata -- --ignored --nocapture` on 2026-10-17 (catalog,
two descriptors, two asset indexes; no game files):

| Version | Catalog SHA-1 (accepted) | Target        | Java | Libraries | Asset objects | Game artifacts | Game bytes  |
| ------- | ------------------------ | ------------- | ---- | --------- | ------------- | -------------- | ----------- |
| 1.21.1  | `499680d6…ace7d`         | Windows x64   | 21   | 70        | 3,888         | 3,961          | 919,845,786 |
| 1.21.1  |                          | Linux x64     | 21   | 56        | 3,888         | 3,947          | 915,051,119 |
| 1.21.1  |                          | macOS x64/arm | 21   | 63        | 3,888         | 3,954          | 918,046,173 |
| 26.3    | `11f1aefe…b2eb`          | Windows x64   | 25   | 74        | 5,147         | 5,224          | 614,675,500 |
| 26.3    |                          | Linux x64     | 25   | 66        | 5,147         | 5,216          | 610,265,329 |
| 26.3    |                          | macOS x64/arm | 25   | 80        | 5,147         | 5,230          | 618,176,824 |

`latest.release` was `26.3` (informational). "Game artifacts" = libraries + asset objects +
client + logging config + asset index. "Missing" bytes in the run exclude the already
verified asset index.

### GUI demonstration

Debug build under Xvfb with a fresh data directory: Installations → New installation shows
"Available from Mojang" from the live catalog; "Estimate download" shows Java major,
library and asset counts, game files and missing bytes, with the runtime size "Determined
when the runtime is prepared" ([1.21.1](verification/p05-plan-1.21.1.png),
[26.3](verification/p05-plan-26.3.png)). Afterwards the data directory held only
`metadata/` (catalog, receipt, two descriptors, two asset indexes; 1.4 MB in total).

### Not verified in P05

- Redirect handling and the public-address resolver against a live redirecting source
  (first exercised by artifact and runtime downloads in P06/P07).
- Game compatibility of either version (P09/P13).

### CI results

Workflow run [38987645015](https://github.com/kurasis/launcher/actions/runs/38987645015) on
commit `827a3b7` (PR #5): all 5 jobs pass. The rules engine, planner and catalog tests ran
on Windows and both macOS architectures; the live metadata test is ignored in CI.

## P06 — Verified, cancellable download engine

- **Host:** development host (Ubuntu 24.04.5 x86_64 container).
- **Branch / PR:** `claude/p06-download-engine`, [kurasis/launcher#6](https://github.com/kurasis/launcher/pull/6).
- **Evidence kind:** local fixture HTTP server (deterministic, in `src-tauri/src/download/tests.rs`)
  plus one live smoke run against the Mojang hosts.

### Fixture suite (18 tests, pass; 15 consecutive runs without failure)

| Case                           | Result                                                                                                                                            |
| ------------------------------ | ------------------------------------------------------------------------------------------------------------------------------------------------- |
| Verify and commit              | Files land at canonical paths only after size + SHA-1 match; staging removed; duplicate requests cause one transfer                               |
| Reuse (MV01)                   | Second job transfers 0 bytes; identical content under another URL is not fetched again                                                            |
| Corruption                     | Wrong bytes: one fresh retry, then `HASH_MISMATCH`; final path never created. Corrupt-then-correct succeeds with one retry                        |
| Wrong size                     | `Content-Length` or body size different from the plan → integrity failure                                                                         |
| Short read / 5xx               | Truncated body then 503 then success: 3 attempts, failed bytes counted in network bytes only. Persistent 503: exactly 1 + 3 attempts              |
| Permanent errors               | 404 → one request, no retry                                                                                                                       |
| `Retry-After`                  | `429` with 1 s honoured; 3600 s fails at once with `RATE_LIMITED` (one request)                                                                   |
| Stall                          | No bytes for longer than the stall timeout → `NETWORK_ERROR` (`transport: stall`), bounded retries                                                |
| Cancel and retry               | Cancel during a stalled transfer returns `OPERATION_CANCELLED` within 2 s; verified file kept, partial file absent, staging removed; retry reuses |
| Redirects                      | Same-source redirect followed; redirect to another host or a loop (more than 5 hops) refused without retry; the foreign target is never contacted |
| Metadata conflicts (MV03/MV04) | Same digest with two sizes and same path with two digests → `METADATA_CONFLICT`; unsafe relative paths rejected; no request made                  |
| Disk space                     | Insufficient space → `DISK_FULL` with required/available bytes before any request                                                                 |
| Existing corrupt file          | Replaced; SHA-256 digests supported (runtime archives)                                                                                            |
| Bounds                         | With concurrency 3 the server never sees more than 3 transfers; progress events ≤ elapsed/interval + 4; progress monotonic and never above total  |
| Receipts                       | One terminal outcome (second `finish` ignored); running receipt → `interrupted` at startup and staging removed; idempotent                        |

Also: `CancelToken` wake-up test, `Retry-After` date parsing, and production routes refusing
URLs outside their source (wrong host, plain HTTP, IP literal) before any connection.

A defect found here was fixed: the redirect limit allowed four hops instead of five
because reqwest's `previous()` includes the original request.

### Live smoke test

`cargo test --test live_download -- --ignored --nocapture` on 2026-10-17: 125 files from the
1.21.1 plan for this host (logging config, the four smallest libraries, 120 asset objects;
6,436,917 bytes) via the production HTTPS transport.

| Step                               | Result                                                                                 |
| ---------------------------------- | -------------------------------------------------------------------------------------- |
| Cancel once 30 files were verified | `OPERATION_CANCELLED`; 35/125 committed (in-flight commits completed); no staging left |
| Retry                              | 35 reused, 90 downloaded, 5,159,602 bytes, 0 retries; 15 progress events               |
| Third run                          | 125 reused, 0 network bytes                                                            |

### Not verified in P06

- Behaviour on Windows/macOS filesystems beyond the CI test run.
- Disk-full during a real write (only the pre-check is exercised; write errors map to
  `DISK_FULL` through the shared I/O mapping).
- Progress display in the UI (P08).

### CI results

Workflow run [38989542569](https://github.com/kurasis/launcher/actions/runs/38989542569) on
commit `60b79ed` (PR #6): all 5 jobs pass. The fixture HTTP suite ran on Windows and both
macOS architectures; the live smoke test is ignored in CI.

## P07 — Private Java runtime installation

- **Host:** development host (Ubuntu 24.04.5 x86_64 container).
- **Branch / PR:** `claude/p07-managed-java`, [kurasis/launcher#7](https://github.com/kurasis/launcher/pull/7).
- **Evidence kind:** automated fixture tests (synthetic archives, fake `java` script) and
  live API/download/execution on the Linux x64 host; other targets live-extracted only.

### Automated tests (25, pass)

| Area                   | Tests                                                                                                                                                                                                                                                                                                                                                                                                                                                                            |
| ---------------------- | -------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------- |
| Provider (4)           | Real API responses select the portable package (Windows: `d35f31e7…1636`, 48,999,141 bytes, as in doc 09); installer `.pkg` never chosen; other major/arch/OS/image type never substituted; bad links, hashes, sizes, names, vendor, JVM and EA releases ignored                                                                                                                                                                                                                 |
| Extractor (8)          | Path normalization; link resolution; files/dirs/modes/internal links/hard links; escaping or absolute links, `..` and absolute entries, FIFOs and missing hard-link targets rejected; no write through a link; dangling links and loops; duplicates and case collisions; entry/file/total limits including a ZIP bomb; ZIP rules; single package root                                                                                                                            |
| Probe (5)              | Property parsing and major/arch checks; Java paths per OS; filtered environment variables; output capped while the pipe keeps draining; hung process killed                                                                                                                                                                                                                                                                                                                      |
| Install flow (5, Unix) | Download + SHA-256 + extract + receipt + probe + promote, notices kept, nothing left in staging; second install reuses without download and re-probes; changed, extra, re-linked file or damaged receipt detected **before execution** and rebuilt from the pinned archive; wrong archive bytes or an escaping link never extract or run Java; wrong major or missing `java` never promoted; JRE, JDK and a second digest with one release label get separate directories (MV05) |
| Reconciliation (1)     | Interrupted promotion restored or cleaned; staging and archives removed                                                                                                                                                                                                                                                                                                                                                                                                          |

### Live checks (2026-10-17)

`cargo test --test live_runtime -- --ignored --nocapture --test-threads=1`:

| Target (host = Linux x64) | Package                                                                                          | Result                                                                                                                                                      |
| ------------------------- | ------------------------------------------------------------------------------------------------ | ----------------------------------------------------------------------------------------------------------------------------------------------------------- |
| Java 21 Linux x64         | `OpenJDK21U-jre_x64_linux_hotspot_21.0.12.1_1.tar.gz`, 52,059,408 bytes, SHA-256 `24131497…b500` | Installed in 22.7 s; probe `java.version=21.0.12.1`, spec 21, `os.arch=amd64`, vendor Eclipse Adoptium; tree receipt `78170dcf…056e`; second install reused |
| Java 25 Linux x64         | `OpenJDK25U-jre_x64_linux_hotspot_25.0.4.1_1.tar.gz`, 61,718,288 bytes, SHA-256 `1731a34b…a219`  | Installed in 27.0 s; probe `25.0.4.1`, spec 25, `amd64`; tree receipt `c94c7b1e…ad87`; second install reused                                                |
| Java 21 Windows x64       | `OpenJDK21U-jre_x64_windows_hotspot_21.0.12.1_1.zip`                                             | SHA-256 verified, extracted: 315 files, 62 dirs, 0 links, 151.5 MB; `bin/java.exe` present; **not executed**                                                |
| Java 25 macOS aarch64     | `OpenJDK25U-jre_aarch64_mac_hotspot_25.0.4.1_1.tar.gz`                                           | SHA-256 verified, extracted: 268 files, 64 dirs, 0 links, 189.6 MB; `Contents/Home/bin/java` present; **not executed**                                      |
| Java 21 macOS x64         | `OpenJDK21U-jre_x64_mac_hotspot_21.0.12.1_1.tar.gz`                                              | SHA-256 verified, extracted: 260 files, 65 dirs, 0 links, 132.5 MB; `Contents/Home/bin/java` present; **not executed**                                      |

All eight major/target combinations resolve to a JRE (no JDK fallback needed).

### Not verified in P07

- Executing the managed Java on Windows and macOS (P13 target machines); CI runs the
  fake-`java` install flow on macOS and the extractor/provider tests on Windows.
- Gatekeeper/quarantine behaviour of an extracted macOS runtime.

### CI results

Workflow run [38989606975](https://github.com/kurasis/launcher/actions/runs/38989606975) on
commit `930324f` (PR #7): all 5 jobs pass. The extractor and provider tests ran on Windows;
the fake-`java` install flow ran on both macOS architectures; live tests are ignored in CI.

## P08 — Complete installation, recovery and repair

- **Host:** development host (Ubuntu 24.04.5 x86_64 container), debug build under Xvfb.
- **Branch / PR:** `claude/p08-install-repair`, [kurasis/launcher#8](https://github.com/kurasis/launcher/pull/8).
- **Evidence kind:** automated orchestration tests (fixture server, synthetic plan, fake
  `java`) and a live GUI install/cancel/resume/repair of Minecraft 1.21.1 on Linux x64.
  No game files are stored in this repository; nothing was launched (P09).

### Automated tests (pass)

| Test                                               | Checks                                                                                                                                                                                                                                                                        |
| -------------------------------------------------- | ----------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------- |
| Fresh install, then a second instance (MV01)       | All artifacts and the runtime verified; lock committed before the summary turns `ready`; asset index projected; permit released; progress phases published; exactly one `operation_finished`; second instance transfers 0 bytes and reuses the runtime; separate game folders |
| Cancel, then resume                                | `cancel_requested` acknowledged, outcome `cancelled`, never `ready`, no lock; a later catalog change is ignored because the frozen plan is reused; files verified before the cancel are not fetched again                                                                     |
| Repair (A15)                                       | One damaged library replaced (1 download), runtime not rebuilt, world, region and options files byte-identical                                                                                                                                                                |
| Failed repair of shared corruption (doc 10 rule 4) | Repair fails (404) → this and every other installation using the damaged file become `repair_required`; their locks are not rewritten                                                                                                                                         |
| Permits and requests                               | Same request ID → same operation; reused ID for another request → `REQUEST_ID_CONFLICT`; second operation → `APP_BUSY` naming the active one; unknown cancel → `OPERATION_NOT_FOUND`; finished cancel → recorded outcome                                                      |
| Crash during install (MV08)                        | A running receipt from a dead process becomes `interrupted` at startup (instance result and recent operations), staging is removed and the next install reuses the frozen plan                                                                                                |
| Asset-index projection (MV06)                      | Same index ID with two hashes alternates correctly; immutable sources untouched; a damaged source is refused                                                                                                                                                                  |
| Large documents (regression)                       | A 6,000-artifact lock and a 6,000-asset frozen plan (> 1 MiB) load as valid                                                                                                                                                                                                   |
| Frontend (6)                                       | Install/Retry/Repair/Verify/Cancel controls, real progress values, busy elsewhere, result texts; store applies newer progress without a revision, drops late progress after the terminal event                                                                                |

### Live GUI run (2026-10-17, Minecraft 1.21.1, Linux x64)

| Step                                | Result                                                                                                                                                                                                                                         |
| ----------------------------------- | ---------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------- |
| Install, then Cancel after ~25 s    | Progress showed 469.8 of 872.2 MiB at 19.7 MiB/s, 2,279 of 3,947 files ([screenshot](verification/p08-install-progress.png)); cancelled, 604 MB of verified files kept, staging removed ([screenshot](verification/p08-install-cancelled.png)) |
| Retry install                       | Finished: 1,004 files (274.8 MiB) downloaded, 2,943 reused; `ready` ([screenshot](verification/p08-install-resumed.png))                                                                                                                       |
| Lock                                | 3,947 artifacts, 915,051,119 bytes, 57 classpath entries, descriptor `499680d6…ace7d`, Java 21 `jdk-21.0.12.1+1` (archive `24131497…`, tree receipt `78170dcf…`)                                                                               |
| Repair with damaged library and JVM | One library byte-patched (SHA-1 changed) and one byte of `lib/libjava.so` in the runtime changed; sentinel `level.dat`, `r.0.0.mca`, `options.txt` created                                                                                     |
| Repair result                       | 2 downloads (the library and the Java archive, 49.8 MiB), 3,946 reused, `ready` ([screenshot](verification/p08-repair-finished.png)); library hash restored; all three sentinel SHA-256 values unchanged                                       |

Defects found by the live run and fixed before this record:

1. The install lock (1.58 MB) and job receipts with a frozen plan (1.6 MB) exceeded the
   1 MiB app-document limit, so they were set aside as corrupt when read back. Locks and
   receipts now use a 64 MiB limit; regression tests cover both. After the fix the app
   correctly refused to trust the earlier `ready` summary without a lock, and a fresh
   install verified everything and downloaded 0 files.
2. Repair rebuilt a valid runtime because the package rebuilt from the lock differs in
   descriptive fields; runtime reuse now compares identity (key, URL, size, format) only.
3. Transfer totals excluded the Java archive; they now include it.

### Not verified in P08

- Windows/macOS live installs (CI runs the orchestration tests on macOS; Windows runs the
  non-Unix parts only).
- Disk-full during a real install, and a real crash (the crash path is tested with a
  synthetic running receipt).

### CI results

Workflow run [39010026310](https://github.com/kurasis/launcher/actions/runs/39010026310) on
commit `0e73f96` (PR #8): all 5 jobs pass. The first run failed on macOS x64: the
download progress-bound test allowed too few events after the new file-check ticker
(two tickers, each with an immediate first tick). The bound was corrected with that
reasoning; it then passed on all runners.

## P09 — Real authenticated Minecraft launch

- **Host:** development host (Ubuntu 24.04.5 x86_64 container), debug build under Xvfb;
  data directory with the P08 installation of 1.21.1 and Temurin 21.0.12.1.
- **Branch / PR:** `claude/p09-launch`, [kurasis/launcher#9](https://github.com/kurasis/launcher/pull/9).
- **Status:** **blocked.** Minecraft was not started: there is no project-owned Microsoft
  client ID, so no real session exists, and the specification forbids substituting a
  fake one. Everything up to the account check is implemented and verified below.

### Automated tests (pass)

| Area                                                      | Tests                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                 |
| --------------------------------------------------------- | ----------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------- |
| Compiler (5)                                              | Both baseline descriptors × Linux/Windows/macOS: no unresolved placeholder, `-Xmx` first, ordered classpath ending in the client, logging argument, expected game arguments; token only in game arguments and never in previews; macOS `-XstartOnFirstThread`; 26.3 native sub-folders; unknown, misplaced and escaping placeholders rejected                                                                                                                                                                                                         |
| Argfile (2), redaction (2)                                | Quoting of backslashes, quotes, `#`, `@`, tabs; non-ASCII refused where required; secrets replaced, including a prefix cut by truncation                                                                                                                                                                                                                                                                                                                                                                                                              |
| Run logs (3)                                              | Rotation keeps 5 bounded files; unwritable directory and `/dev/full` degrade without blocking; pruning keeps the newest runs within count and size, never the active run                                                                                                                                                                                                                                                                                                                                                                              |
| Encoding guard (1)                                        | Substituted characters detected in UTF-8 and Windows-ANSI output                                                                                                                                                                                                                                                                                                                                                                                                                                                                                      |
| Launch service (7, fake `java` that echoes its arguments) | Supervised run recorded with exit code, redacted tail and log, run folder removed, permit released, `last_played_at` set; JVM arguments via argfile with the token still direct; no spawn without verified access (`AUTH_NOT_CONFIGURED`, `NO_GAME_ACCESS`, `SERVICE_UNAVAILABLE`); missing file → `REPAIR_REQUIRED`, permit released, Repair then Play works; same request ID → same run, second launch → `GAME_ALREADY_RUNNING`, install/repair → `APP_BUSY`, Stop kills the owned process; exit code 3 → failure; cancel before spawn → no process |
| Frontend (4)                                              | Play calls `launch_game` and shows the reason it cannot start; Stop game asks first with the unsaved-world warning; degraded logging and "Not started" labels                                                                                                                                                                                                                                                                                                                                                                                         |

The fake `java` is a test double for the supervisor only; production always uses the
account service.

### Live checks with the real managed Java (2026-10-17)

`KURASIS_LIVE_ROOT=<data dir> cargo test --test live_launch_args -- --ignored --nocapture --test-threads=1`:

| Check                                               | Result                                                                                                                                                                                                                                                                              |
| --------------------------------------------------- | ----------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------- |
| Argument echo (Temurin 21.0.12.1, `LC_ALL=C.UTF-8`) | 6 values (Windows path with spaces, embedded quote, leading `#`, leading `@`, trailing backslash, `Jürgen игры 世界`) round-trip unchanged via argfile and via direct arguments                                                                                                     |
| Same under `LC_ALL=POSIX`                           | Non-ASCII arrives as `?`; the preflight guard detects it (would refuse with `UNSUPPORTED_PATH_ENCODING`)                                                                                                                                                                            |
| Real 1.21.1 JVM arguments                           | Java 21.0.12.1 accepted all 10 compiled JVM arguments from an argfile; `java.class.path` has the 57 locked entries, all present; `java.library.path` is the run's natives folder; log4j configuration path resolved. `-version` replaced the main class, so Minecraft did not start |
| GUI: Play without sign-in                           | Preflight passed (lock, 3,947 files, runtime tree, metadata), then the account check refused: "Microsoft sign-in is not configured…"; run shown as "Not started"; no `java` process ([screenshot](verification/p09-play-without-sign-in.png))                                       |

### Done-when items (blocked)

| Item                                                             | Status                                                                                                        | Needed                                         |
| ---------------------------------------------------------------- | ------------------------------------------------------------------------------------------------------------- | ---------------------------------------------- |
| Real game reaches the menu; create, save and reload a test world | **Blocked**                                                                                                   | Project-owned client ID, licensed test account |
| Authorized online-mode server accepts the session                | **Blocked**                                                                                                   | As above, plus a test server                   |
| Duplicate launch rejected; no-access never spawns Java           | Verified with the supervisor test double and the GUI (no sign-in)                                             | Reconfirm with a real account (P13)            |
| Spaces/Unicode/long arguments; bounded, redacted logs            | Verified with real Java (argument echo, encoding guard) and automated tests; Windows/macOS locales unverified | P13 target machines                            |

### CI results

Recorded by the next phase's pull request.

## P10 — Unload GUI during play and return after exit

- **Host:** development host (Ubuntu 24.04.5 x86_64 container), debug build under Xvfb +
  openbox; data directory with the installed 1.21.1.
- **Branch / PR:** `claude/p10-background`, [kurasis/launcher#10](https://github.com/kurasis/launcher/pull/10).
- **Status:** **blocked** for the real-game items (no sign-in → no Minecraft run, as in
  P09). The game path uses the same window controller as the P03 harness, whose 12
  scenarios and 40 unload/restore cycles passed on this host (P03 section).

### Implemented here

- Game runs drive the P03 window controller (unload after 1.5 s survival in the default
  mode; keep-open mode only updates the existing window; one window restored after exit).
- A background install/repair that finishes with no window brings one window back with
  its result, unless the user chose cancel-and-quit.
- Quit while busy: games keep the P03 choice (keep in background / confirmed stop and
  quit); installs and repairs offer keep in background / cancel and quit; a launch still
  in preflight is cancelled. The exit waits at most 30 s for the work to record its result.
- Supervisor restart: `locks/active-run.json` stores the game's PID, OS start time and
  executable. At startup a matching live process holds the activity permit (no launch,
  install or repair) until it exits, with an explanatory run record; Stop re-checks the
  identity before ending it. A PID that now belongs to another process is never signalled
  and the stale record is removed.
- No tray icon: reopening the launcher (second invocation, macOS Dock reopen) is the
  documented way back; it does not depend on a tray extension.

### Automated tests (pass)

| Area                            | Tests                                                                                                                                                                                                    |
| ------------------------------- | -------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------- |
| Active-run record (3)           | Live recorded process is a survivor until it exits; reused PID (other start time or executable) is never treated as the game or signalled; clearing only removes the own record                          |
| Launch service (2 new/extended) | Record written while a game runs and cleared after exit; a survivor blocks launch (`GAME_ALREADY_RUNNING`) and repair (`APP_BUSY`), Stop ends it after re-checking identity, then everything is released |
| Frontend (1)                    | Quit dialog wording and "Cancel and quit" for an install/repair                                                                                                                                          |

### Live GUI scenarios (Linux x64 host)

| Scenario                                                                                | Result                                                                                                                                |
| --------------------------------------------------------------------------------------- | ------------------------------------------------------------------------------------------------------------------------------------- |
| Verify files, close the window                                                          | Window gone, supervisor alive, job continued (`windows=0 supervisors=1`)                                                              |
| Second launcher invocation during the job                                               | Exactly one window recreated showing live progress ([screenshot](verification/p10-reopened-during-job.png))                           |
| Close again, wait                                                                       | Job finished in the background; one window returned by itself with the result ([screenshot](verification/p10-returned-after-job.png)) |
| Quit during a repair → Cancel and quit ([dialog](verification/p10-quit-during-job.png)) | Repair recorded once as `cancelled`, installation `needs_verification`, process exited 3.6 s later, no window reopened                |
| Quit while idle                                                                         | Process exited within 0.1 s (not trapped)                                                                                             |

### Done-when items

| Item                                                                         | Status                                                                            |
| ---------------------------------------------------------------------------- | --------------------------------------------------------------------------------- |
| Real Minecraft survives GUI destruction and returns to the same installation | **Blocked** (no sign-in); same controller verified with the P03 harness           |
| Crash/early exit restores one usable error view                              | Verified with the P03 harness and the launch supervisor tests; real crash blocked |
| Second invocation restores the existing supervisor                           | Verified (during a background job)                                                |
| Idle quit not trapped; tray absence does not strand the app                  | Verified on Linux X11; Windows/macOS/Wayland in P13                               |

### CI results

Recorded by the next phase's pull request.
