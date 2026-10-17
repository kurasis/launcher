# Verification record

Actual results only. Each entry names the host, the commit or pull request and the kind of
evidence: **static** (lint/type/compile), **automated** (tests with fixtures or mock
runtime), **host GUI** (app run on the development host), **CI** (GitHub-hosted runners),
**live API** or **real game**. Nothing here is a target-machine qualification unless it
says so; the qualification matrix is completed in P13.

From P04 on, a phase's **CI results** are recorded by the following phase's pull request,
because adding them to the same pull request would start another full CI run. The phase
is merged only after its own required checks pass.

Entries recorded before [D-031](DECISIONS.md#d-031--neutral-application-identity) quote
the provisional names in use at the time (product, binary and package names, environment
variables, the test player name); screenshots show them too. Commands below use the
current `LAUNCHER_` names.

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
  (`npx tauri build --debug --no-bundle`), `LAUNCHER_DATA_ROOT` pointing to a scratch
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

`LAUNCHER_LIVE_ROOT=<data dir> cargo test --test live_launch_args -- --ignored --nocapture --test-threads=1`:

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

Workflow run [39010670846](https://github.com/kurasis/launcher/actions/runs/39010670846) on
commit `9102f08` (PR #9): all 5 jobs pass. The launch-service tests with the test-double
`java` ran on Linux and both macOS architectures; compiler, argfile, redaction, log and
encoding tests also ran on Windows.

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

Workflow run [39011719689](https://github.com/kurasis/launcher/actions/runs/39011719689) on
commit `5241284` (PR #10): all 5 jobs pass. The first run on this pull request failed on
macOS x64 only: a test observed the finished run before the active-run record was
cleared. The exit callback now clears the record and the handle before publishing the
result; the fix is in the merged code.

## P11 — Complete web UI, settings and diagnostics

- **Host:** development host (Ubuntu 24.04.5 x86_64 container), debug build under Xvfb.
- **Branch / PR:** `claude/p11-product-ui`, [kurasis/launcher#11](https://github.com/kurasis/launcher/pull/11).
- **Status:** **blocked** only for the end-to-end first-use journey, which stops at
  Microsoft sign-in (P04). All screens and controls are wired to real commands.

### Automated tests (pass)

| Area                        | Tests                                                                                                                                                                                                                                                  |
| --------------------------- | ------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------ |
| Diagnostic export (Rust, 2) | Sanitizer removes home/data paths, JWTs, `--accessToken`, `refresh_token=`, `Authorization: Bearer`; the archive contains exactly README, summary, jobs and the run's `output.log` (redacted), never `accounts.json`, the vault key or a run's argfile |
| External links (Rust)       | Source link allowed only for `github.com/kurasis/launcher[/…]`; other repositories and look-alike paths refused                                                                                                                                        |
| Frontend (6 new)            | Diagnostics table and export (saved / cancelled); defaults form saves through `set_app_settings`; navigation to Diagnostics and the source link via `open_external`; header indicator shows "Installing 45%" on every screen                           |

Frontend total: 54 tests; Rust unit tests: 244.

### GUI checks (Linux x64 host)

| Check                                 | Result                                                                                                                                                                                                                       |
| ------------------------------------- | ---------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------- |
| Keyboard                              | Tab reaches the navigation with a visible focus ring; Enter opens Diagnostics ([screenshot](verification/p11-diagnostics-keyboard-focus.png)); the Quit and Stop dialogs focus the safe choice and close with Escape (tests) |
| Export through the native save dialog | `diag.zip` written (1,951 bytes): `README.txt`, `summary.json`, `jobs.json`; no data-directory path, no account data                                                                                                         |
| Settings                              | Theme, during-game mode, defaults and read-only data location ([screenshot](verification/p11-settings.png))                                                                                                                  |
| Window                                | 1100 × 720 default, 820 × 560 minimum (configuration); reduced-motion rule in the design tokens                                                                                                                              |

### Not verified in P11

- 200 % text scaling and screen readers (P13 on target desktops).
- Open game folder with a desktop file manager (the container has none).
- The full first-use journey through sign-in and Play (blocked by P04).

### CI results

Workflow run [39018163000](https://github.com/kurasis/launcher/actions/runs/39018163000) on
commit `a2e2555` (PR #11): all 5 jobs pass. The previous run failed once on linux-x64:
the surviving-game test saw its own `sleep` process as gone because Linux reported its
start time a second differently (the kernel boot time moves with clock corrections).
Start times now match within 5 seconds together with PID and executable (D-026).
The orphan tests failed again on
[kurasis/launcher#18](https://github.com/kurasis/launcher/pull/18): a freshly recorded
`sleep` process did not match itself. The tests read the child's executable from
`/proc` immediately after spawning it, which can still name the parent before the
child's `exec` completes; they now wait for the `exec` and print both identities on
failure. Independently, Linux start times are now counted from boot with the boot ID,
so clock corrections no longer move them (D-026).

## P12 — Windows, macOS and Linux packages

- **Hosts:** packages built in a Debian 12 chroot (glibc 2.36) on the development host;
  tested in freshly bootstrapped Debian 12.15 and Ubuntu 24.04 minimal roots (chroot,
  Xvfb display), not on consumer machines. Windows and macOS packages are built and
  smoke-tested only by the `Package` workflow in CI.
- **Branch / PR:** `claude/p12-packaging`.
- **Status:** **blocked** for the done-when items that need clean target machines (P13)
  and owner signing credentials; everything else is implemented and verified as below.

### Local Linux packages (unsigned)

Built from commit `04898f0` plus the uncommitted P12 changes (`dirtyWorkingTree: true` in
`build-info.json`); CI rebuilds them from the pushed commit.

| Artifact                                | Bytes       | SHA-256                                                            |
| --------------------------------------- | ----------- | ------------------------------------------------------------------ |
| `Kurasis Launcher_0.1.0_amd64.deb`      | 6,130,524   | `74d3af127d613b1626d953f949703b1d8ea580efe200cb15b102c518cab9dd2f` |
| `Kurasis Launcher_0.1.0_amd64.AppImage` | 101,640,696 | `1a8b016df6b1e94448eb2217d72c5cc4682c0cfa97fa7ca00085b52fb00b6e15` |

Toolchain: rustc 1.97.0, Node 24.21.0, Tauri CLI 2.12.3, glibc 2.36. The binary needs
glibc ≥ 2.34. Declared `.deb` dependencies: `ca-certificates, libdbus-1-3,
libwebkit2gtk-4.1-0, libgtk-3-0`.

### Smoke tests (`scripts/smoke/linux.sh`, pass on both systems)

| Step                                                                 | Debian 12 | Ubuntu 24.04 |
| -------------------------------------------------------------------- | --------- | ------------ |
| `.deb` installs with declared dependencies only; no Rust/Node/Java   | pass      | pass         |
| Installed launcher runs 20 s under Xvfb and holds the DATA_ROOT lock | pass      | pass         |
| `apt purge` removes the program and keeps user data                  | pass      | pass         |
| AppImage starts on a stripped system plus host desktop libraries     | pass      | pass         |

Found and fixed while testing:

- On a fresh Debian 12 without `ca-certificates` the launcher panicked at startup ("The
  HTTPS client could not be created"). The package now depends on `ca-certificates` and
  `libdbus-1-3`; startup failures exit with a readable message instead of a panic.
- The AppImage needs the libraries linuxdeploy leaves to the host (ALSA, EGL/GL/GLES,
  GBM, fontconfig, FriBidi, HarfBuzz, Wayland client); the first runs failed without
  them. Documented in INSTALL.md and D-028.
- Packaged builds said "Development build"; the label now follows the Rust core's build
  profile: "Qualification build" for release cores
  ([screenshot](verification/p12-packaged-debian12.png)), "Development build" for debug
  cores even when their frontend is bundled.

### Upgrade and uninstall (Debian 12, real version change)

A temporary 0.1.1 build (version bump only, not committed) upgraded an installed 0.1.0
with an existing data root: an installed Minecraft 1.21.1 instance, a test world
(`saves/Upgrade World/level.dat`, `region/r.0.0.mca`), `options.txt` and changed settings
(dark theme, default heap 3072 MiB).

| Check                                                     | Result                                                        |
| --------------------------------------------------------- | ------------------------------------------------------------- |
| 0.1.0 starts with the data; settings applied              | pass (dark theme, instance listed)                            |
| `apt install` of 0.1.1 over 0.1.0                         | "Unpacking kurasis-launcher (0.1.1) over (0.1.0)"             |
| 0.1.1 starts and reports version 0.1.1                    | pass ([screenshot](verification/p12-upgrade-about-0.1.1.png)) |
| Settings, world, options and instance files after upgrade | all 8 files byte-identical (SHA-256)                          |
| `apt purge` after the upgrade                             | program removed; all 8 files still byte-identical             |

### CI package workflow

`.github/workflows/package.yml` builds the NSIS installer (windows-2025), DMGs
(macos-15 arm64, macos-15-intel x64) and the Linux packages (debian:12), smoke-tests them
and uploads them as workflow artifacts. Results are recorded by the next phase's pull
request.

### Not verified in P12

- Clean consumer Windows 10/11, macOS and Linux desktops, Wayland and HiDPI (P13).
- Missing WebView2 on Windows (bootstrapper download) on a real machine.
- Signed packages: no Apple Developer ID or Windows code-signing certificate exists
  (owner action, docs/PACKAGING.md). All packages are unsigned.
- Upgrade on Windows and macOS (CI installs a single version).

### CI results

Commit `d200ec0` (PR #12), all 11 jobs pass:
[CI 39020749073](https://github.com/kurasis/launcher/actions/runs/39020749073) (5 jobs) and
[Package 39020749070](https://github.com/kurasis/launcher/actions/runs/39020749070) (6 jobs).
Earlier runs on this pull request failed the Linux smoke jobs with status 141: `head -1`
closed the pipe before `ldd` finished under `pipefail`. Fixed in the script.

| Job                         | Runner system                                  | Result                                                                                                                                                                                                                                             |
| --------------------------- | ---------------------------------------------- | -------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------- |
| Build and smoke windows-x64 | Windows Server 2025 (10.0.26100), WebView2 154 | `Kurasis Launcher_0.1.0_x64-setup.exe`, 3,662,855 bytes, SHA-256 `66d123de5120bd397a3182a7e46c18e52d25edc2a7cd3ef17c78080c2362e2a2`, unsigned; silent per-user install, launcher running after 20 s with DATA_ROOT, silent uninstall kept the data |
| Build and smoke macos-arm64 | macOS 15.7.9 (24G830), arm64                   | DMG mounts; `Mach-O 64-bit executable arm64`, minimum 11.0, signature ad-hoc (linker), no team; running after 20 s; deleting the app kept the data                                                                                                 |
| Build and smoke macos-x64   | macos-15-intel                                 | Pass (same steps)                                                                                                                                                                                                                                  |
| Build linux-x64             | debian:12 container                            | `.deb` and AppImage with `SHA256SUMS` and `build-info.json`                                                                                                                                                                                        |
| Smoke linux-x64             | fresh debian:12 and ubuntu:24.04 containers    | Pass (same steps as the local roots above)                                                                                                                                                                                                         |

Each artifact set holds its own `SHA256SUMS` and `build-info.json`; pull-request artifacts
expire after 7 days. These are CI runners, not consumer machines.

## P13 — Compatibility, security and performance qualification

- **Date / host:** 2026-10-17; development host (Ubuntu 24.04.5 container, 4 vCPU Intel
  Xeon @ 2.10 GHz, 15 GiB RAM, Xvfb + openbox, X11 only); packaged runs in a Debian 12
  chroot. App commit `e06b52a` plus the P13 changes on `claude/p13-qualification`.
- **Status:** **blocked.** Everything that needs a signed-in Microsoft account (sign-in,
  Play, worlds, servers) and every check that needs real Windows, macOS, Wayland or Apple
  silicon/Intel machines is still open. Nothing below relabels fixture or host results as
  real-machine or real-account results.

### Second baseline and shared cache (live, Linux x64)

`tests/live_install.rs` (new, ignored by default) runs the real install service against
Mojang and Adoptium in a given data directory. Both descriptors match the live manifest
(1.21.1 `499680d6…`, 26.3 `11f1aefe…`, Java 21 and 25).

| Step (one data directory)                                                        | Result                                                                                           |
| -------------------------------------------------------------------------------- | ------------------------------------------------------------------------------------------------ |
| Install 26.3 next to an installed 1.21.1                                         | Succeeded in 57.7 s: 5,020 files downloaded (568.8 MB), 197 reused                               |
| Repair 1.21.1, then 26.3 (alternating)                                           | 0 bytes downloaded each; 3,947 and 5,216 files reused; cache size unchanged                      |
| Second installation of 1.21.1 (MV01)                                             | 0 bytes downloaded, 3,947 reused; separate empty game folder                                     |
| Real JVM arguments, 26.3 with Java 25.0.4.1                                      | All 14 accepted; 67 classpath entries present; `java.library.path` = `${natives_directory}/java` |
| Real JVM arguments, 1.21.1 with Java 21.0.12.1                                   | All 10 accepted; 57 classpath entries present                                                    |
| Data root at a 221-character path with spaces, `Jürgen Ä 世界 игрок` (A16, MV12) | Repair of both versions and both real-Java argument checks pass                                  |

Found and fixed: the live argument test assumed the natives directory itself as library
path; 26.3 names a subdirectory. The test now takes the expected value from the compiled
arguments.

### Failure injection (A22)

| Failure                                                      | Observed outcome                                                                                                             |
| ------------------------------------------------------------ | ---------------------------------------------------------------------------------------------------------------------------- |
| No network (`unshare -n`)                                    | `NETWORK_ERROR` "A download source could not be reached…" after 0.5 s, retryable, nothing written as ready                   |
| Data root on a 64 MiB filesystem                             | `DISK_FULL` before any artifact download: 1,183,037,018 bytes required, 64,716,800 available; 2.3 MB metadata written        |
| Read-only data root                                          | `PERMISSION_DENIED` "Creating the launcher data directory: permission denied by the operating system."                       |
| `kill -9` 25 s into a 26.3 install, then retry               | Job receipt reconciled to `interrupted`; retry succeeded in 39.1 s reusing 2,753 verified files; no `.part` files left       |
| Corrupted library jar and corrupted `libjvm.so`, then Repair | Repair downloaded 2 files (the jar and the Java archive, 61.9 MB), reused 5,215; jar listed as corrupt; runtime re-extracted |
| Expired token                                                | Covered by auth fixture tests only (no real account)                                                                         |

Found and fixed: the free-space check asked sysinfo's disk list, which leaves out tmpfs
and similar filesystems, so a data root there was measured against its parent mount and
the install ran into `StorageFull` mid-download. The check now uses `statvfs` of the data
root itself on Unix (D-029). The new unit test then failed on Windows CI: there the check
had never worked, because the canonical `\\?\D:\…` path never matched a volume such as
`D:\`. Windows now matches the absolute path without the verbatim prefix.

### Multiple processes (MV09)

| Case                                              | Result                                                                                                                                                                                                  |
| ------------------------------------------------- | ------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------- |
| Second packaged launch with the same session bus  | Exits at once (code 0); one process, one window                                                                                                                                                         |
| Second launch without a shared bus (signal fails) | Second process does not touch the data: "Launcher data is unavailable — another launcher process is already using the launcher data directory" ([screenshot](verification/p13-mv09-second-process.png)) |

### Lifecycle stress (A23, test child)

20 unload/return cycles with the debug harness ("Run 4 seconds", default mode), current
code ([CSV](verification/p13-lifecycle-cycles.csv)): 20/20 unloaded and restored, exactly
one window and one supervisor after each cycle, no leftover child, WebKit process count
constant (3), supervisor threads stable (29), RSS 193.4 → 216.1 MiB (growth flattening,
≈ 0.1 MiB per cycle in cycles 9–20).

Found, not fixed: each webview destroy/recreate leaves one `timerfd` and one
`WebKitSharedMemory` memfd open in the supervisor (+2 descriptors per cycle, 45 → 83). In
"Keep the launcher open" mode, six cycles kept the count flat (42), so the descriptors
belong to WebKitGTK's webview teardown, not to child supervision (pipes and pidfds are
closed). With a soft limit of 1,024 descriptors this allows about 490 unload cycles per
launcher run. Recorded as a known limitation; Windows and macOS are unmeasured.

### Performance (packaged release build, Debian 12 chroot, Xvfb, software rendering)

| Measurement                                                                                       | Result             |
| ------------------------------------------------------------------------------------------------- | ------------------ |
| Process start to visible window, 3 runs                                                           | 155, 134, 119 ms   |
| Whole process tree (launcher, WebKitWebProcess, WebKitNetworkProcess), PSS after 10 s / 70 s idle | 347 / 345 MiB      |
| Idle CPU of the whole tree over 60 s                                                              | 0.11 % of one core |

Method: shell measurement (`/proc/<pid>/smaps_rollup` PSS summed over the tree; utime +
stime over 60 s; window visibility polled with `xdotool`). Xvfb forces Mesa software rendering, which inflates
WebKit memory; numbers on real GPUs and other OSes are not measured.

### Security checks

| Check                                                                      | Result                                                                                                                                                         |
| -------------------------------------------------------------------------- | -------------------------------------------------------------------------------------------------------------------------------------------------------------- |
| Release binary contains no harness, fixtures or test-only transports       | `kurasis-lifecycle-test-child`, `127.0.0.1`, `KURASIS_DATA_ROOT`, fixture names: absent; `start_lifecycle_test` only in ACL text without a handler (as in P03) |
| Unsafe archives (traversal, absolute paths, links out of the tree, limits) | Extractor tests (P07) pass on all CI targets                                                                                                                   |
| Tokens never in frontend, logs, argfile previews or exports                | Redaction, argfile and export tests pass; no real token exists to test end to end                                                                              |
| Production auth has no bypass                                              | Play refuses without a session (P09 GUI); no session fixture compiled into release                                                                             |

### Acceptance checklist (A01–A26)

| Item | Status  | Evidence / blocker                                                                                                                              |
| ---- | ------- | ----------------------------------------------------------------------------------------------------------------------------------------------- |
| A01  | Partial | Fresh Debian 12/Ubuntu 24.04 roots and CI runners (P12); consumer machines not tested                                                           |
| A02  | Partial | Linux dependencies verified (P12); Windows uses the WebView2 bootstrapper (config + CI runner with WebView2); missing-WebView2 machine untested |
| A03  | Blocked | No project-owned Microsoft client ID (P04)                                                                                                      |
| A04  | Blocked | As A03                                                                                                                                          |
| A05  | Blocked | As A03; no Game Pass test account                                                                                                               |
| A06  | Partial | Distinct outcomes in fixture tests (P04); live check blocked                                                                                    |
| A07  | Partial | Redaction/export/argfile tests; no real token yet                                                                                               |
| A08  | Partial | Vault and session-only fallback tests (P04); real sign-in blocked                                                                               |
| A09  | Pass    | Live catalog filtered by the compatibility list (P05); instances keep their pinned descriptor                                                   |
| A10  | Pass    | Live installs of 1.21.1 (P08) and 26.3 (above) with digest verification and private Java                                                        |
| A11  | Pass    | 1.21.1 → Java 21.0.12.1, 26.3 → Java 25.0.4.1, both accepting real JVM arguments                                                                |
| A12  | Partial | Demo mode with the test profile on Linux X11: menu, world created, saved, reloaded (D-030); licensed play on every target blocked by sign-in    |
| A13  | Blocked | Needs sign-in and an authorized test server                                                                                                     |
| A14  | Pass    | Cancel/resume (P08) and `kill -9` resume (above)                                                                                                |
| A15  | Partial | Library and runtime corruption repaired (above, P08); worlds unchanged (P08); repair blocked while a game runs: unit tests only                 |
| A16  | Pass    | Unicode/space/221-character data root (above); classpath-delimiter roots rejected (P02)                                                         |
| A17  | Partial | Real 1.21.1 kept running after the GUI was destroyed, one supervisor (test profile, demo); Windows/macOS untested                               |
| A18  | Partial | Real game: webview destroyed during play and one window restored after normal exit and after a crash (test profile, demo, Linux X11)            |
| A19  | Partial | Linux X11 host (P03, P10); Windows/macOS/Wayland untested                                                                                       |
| A20  | Pass    | Bounded logs, drained pipes, degraded logging (P09 tests, P03 heavy-output scenario)                                                            |
| A21  | Partial | All controls call real commands (P11); sign-in/Play paths cannot complete                                                                       |
| A22  | Partial | Network, disk full, permissions, corruption and abrupt exit pass (above); expired token fixture-only                                            |
| A23  | Partial | 20 test-child cycles; real-game exit and crash return on Linux X11 (demo); WebKitGTK descriptor growth documented; other targets open           |
| A24  | Partial | Linux upgrade/uninstall keeps data (P12); Windows/macOS upgrade untested; credentials need sign-in                                              |
| A25  | Pass    | Lockfile-enforced CI and package builds (P01–P12)                                                                                               |
| A26  | Partial | Package metadata and checksums (P12); final signed packages and target smoke tests pending                                                      |

### MV01–MV12

| Case | Status  | Evidence                                                                                                             |
| ---- | ------- | -------------------------------------------------------------------------------------------------------------------- |
| MV01 | Pass    | Second 1.21.1 installation: 0 bytes, own game folder (above)                                                         |
| MV02 | Partial | Both versions launched with their own Java (1.21.1/GLX, 26.3/Vulkan) in demo mode; alternating licensed play blocked |
| MV03 | Pass    | Fixture tests (P05, P06)                                                                                             |
| MV04 | Pass    | Fixture tests (P05, P06)                                                                                             |
| MV05 | Pass    | Runtime key tests (P07)                                                                                              |
| MV06 | Pass    | Asset-index projection tests (P05, P08)                                                                              |
| MV07 | Partial | Corruption repaired once for both users of the file (above); APP_BUSY during a running game: unit tests              |
| MV08 | Pass    | Interruption tests (P02, P08) and live `kill -9` resume (above)                                                      |
| MV09 | Pass    | Second process exits or fails closed (above)                                                                         |
| MV10 | Partial | Background install return and cancel-and-quit (P10); login-time close blocked                                        |
| MV11 | Pass    | Degraded-logging tests with heavy output (P09)                                                                       |
| MV12 | Partial | Unicode/space paths with real Java (above); cancel races in unit tests; real spawn blocked                           |

### Not verified in P13

- Every real-account item (A03–A08, A12, A13) and real game play, save, exit and crash.
- Windows 10/11, macOS on Apple silicon and Intel, Linux Wayland, HiDPI and 200 % text.
- Signed packages and final-package rebuild and smoke tests (owner signing credentials).

### CI results

Recorded by the next phase's pull request.

## P14 — Handover and reproducibility

- **Date / host:** 2026-10-17, development host (Ubuntu 24.04.5 x86_64 container).
- **Status:** **blocked.** The source, documentation and build reproduction are complete;
  a user cannot yet sign in and play, and P13 is not passed, so the MVP is not claimed.

### Clean-checkout reproduction (pass)

A fresh `git clone` of `claude/p13-qualification` at `057a999` into an empty directory,
then exactly the commands from README/DEVELOPMENT.md with Node 24.21.0 and the pinned Rust
1.97.0:

| Command                                   | Result                                                                                                                   |
| ----------------------------------------- | ------------------------------------------------------------------------------------------------------------------------ |
| `npm ci`                                  | 236 packages from `package-lock.json`                                                                                    |
| `npm run check`                           | typecheck, lint, format, 55 frontend tests, rustfmt, clippy, 245 Rust unit tests + integration tests; live tests ignored |
| `npx tauri build --no-bundle -- --locked` | Release binary built in 6 m 22 s (14,832,728 bytes)                                                                      |
| `git status` afterwards                   | Clean: the build changes no tracked file                                                                                 |

Installer packages are reproduced by the `Package` workflow from the same lockfiles
(P12).

### Source deliverable check (pass)

282 tracked files. No archives, jars, Java runtimes, game files, packages, keys or
environment files are tracked; a pattern search for JWTs, private keys, GitHub/AWS tokens
and client secrets finds nothing. The Microsoft client ID is not in the repository (build
configuration only).

### Documentation

| Audience   | Documents                                                                                                               |
| ---------- | ----------------------------------------------------------------------------------------------------------------------- |
| Users      | [INSTALL.md](INSTALL.md), [TROUBLESHOOTING.md](TROUBLESHOOTING.md), [RELEASE-NOTES.md](RELEASE-NOTES.md)                |
| Owner      | [AUTH-SETUP.md](AUTH-SETUP.md) (Microsoft application, service access), [PACKAGING.md](PACKAGING.md) (signing secrets)  |
| Developers | [README.md](../README.md), [DEVELOPMENT.md](DEVELOPMENT.md) (incl. live tests), [DECISIONS.md](DECISIONS.md), this file |

Stale statements removed: README "P01" status, the lifecycle panel location (now
Diagnostics), "PKCE to implement" in AUTH-SETUP.

### Not done

- A user cannot sign in, play and return (Microsoft client ID, P04).
- No final package checksums: final packages must be rebuilt from the qualified commit
  and smoke-tested on target machines after P13; no release was published (separate
  owner decision).

### CI results

Recorded by the pull request that merges this phase.

## Test profile — automated launch tests in demo mode (D-030)

- **Date / host:** 2026-10-17, development host (Ubuntu 24.04.5 container, Xvfb, Mesa
  25.2.8 llvmpipe/lavapipe), debug builds of branch `claude/test-profile`.
- **Label:** every result below is **test profile, demo mode**. No Microsoft account was
  used; none of this is account evidence (A03–A06, A13 stay open).

### Automated: `tests/live_play.rs` (real game through the launch service)

| Version | Graphics         | Process started | Resources loaded | Stop                                |
| ------- | ---------------- | --------------- | ---------------- | ----------------------------------- |
| 1.21.1  | GLX, llvmpipe    | 3.7 s           | 12.7 s           | Run recorded as `stopped`, no error |
| 26.3    | Vulkan, lavapipe | 4.6 s           | 16.1 s           | Run recorded as `stopped`, no error |

Each run checks that the profile name reached the client (`Setting user: KurasisTest`)
and that the placeholder token does not appear in the run log. Screenshots: 1.21.1 demo
title screen ([png](verification/test-profile-1.21.1-title.png)), 26.3 first-start
screen ([png](verification/test-profile-26.3-first-start.png)).

Found while getting there:

- Stop was recorded as "Exited with an error" (`failed`). Runs now end as `stopped`
  ("Stopped by you"); the instance's last result is "Cancelled".
- 26.3 could not create an OpenGL window on Xvfb or the Xorg dummy driver ("Couldn't find
  matching GLX visual"; neither offers sRGB-capable visuals). With the Mesa Vulkan driver
  it selects its Vulkan backend and runs. Environment requirement, not a launcher defect.

### End to end through the app (1.21.1, default "unload and return" mode)

| Step                                               | Result                                                                                                                                         |
| -------------------------------------------------- | ---------------------------------------------------------------------------------------------------------------------------------------------- |
| Debug app with `KURASIS_TEST_PROFILE=KurasisTest`  | Header shows "Test profile KurasisTest · demo mode" ([png](verification/test-profile-tp-1-home.png))                                           |
| Play                                               | Launcher window unloaded after 6 s; game window after 11 s; title screen after 21 s                                                            |
| Play Demo World                                    | Integrated server started, 1,399 advancements loaded, player in the world ([png](verification/test-profile-tp-3-world.png))                    |
| Save and Quit to Title                             | "All dimensions are saved"; `saves/Demo_World/level.dat` written ([png](verification/test-profile-tp-5-pause.png))                             |
| Play Demo World again (reload)                     | Saved world loaded, "KurasisTest joined the game" ([png](verification/test-profile-tp-7-reloaded-ingame.png))                                  |
| Save and Quit, then Quit Game                      | Saved again; one launcher window back 1 s after the game exited, no game process left                                                          |
| Result view                                        | "Exited normally", exit code 0, 400.2 s; last result "Launch: Succeeded (exit code 0)" ([png](verification/test-profile-tp-8-returned.png))    |
| Play again, then `kill -9` the game's Java process | One launcher window back immediately; "Exited with an error" with the explanation and output ([png](verification/test-profile-tp-9-crash.png)) |

This covers, in demo mode on Linux X11: A12 (menu, create/save/reload a world), A17 and
A18 (GUI destroyed during play, game unaffected, one window restored after normal exit
and after a crash) and the launch side of MV02 (each version with its own Java).
Windows, macOS and Wayland remain open.

### Release builds

A release core built from this branch (`npx tauri build --no-bundle -- --locked`)
contains none of `KURASIS_TEST_PROFILE`, `kurasis-test-profile`, `KurasisTestProfile`,
`--demo` or `lifecycle-test-child` (`strings` scan, 0 matches each); `get_app_info`
returns `testProfile: null` in release builds by construction.

## Neutral identity (D-031)

- **Date / host:** 2026-10-17, development host (Ubuntu 24.04.5 container, Xvfb, Mesa
  25.2.8), branch `claude/neutral-naming`.

| Check                                                    | Result                                                                                                                                                                                                                                                                           |
| -------------------------------------------------------- | -------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------- |
| `npm run check`                                          | Pass: 58 frontend tests, 248 Rust unit tests and the integration tests                                                                                                                                                                                                           |
| Release core (`npx tauri build --no-bundle -- --locked`) | Binary `src-tauri/target/release/launcher`. `strings` scan: the old name appears only in the repository address `github.com/kurasis/launcher`; `LAUNCHER_NOTOKEN_PROFILE`, `notoken-profile`, `NoTokenProfile`, `LAUNCHER_DATA_ROOT`, `lifecycle-test-child` and `--demo` absent |
| Existing development data after rename                   | Launch refused with "Some game files are missing or damaged" because the Java receipt is now `.launcher-runtime.json`; `live_install` repair re-downloaded only the runtime archive (1.21.1: 1 file, 52 MB, 3,947 reused; 26.3: 1 file, 62 MB, 5,216 reused)                     |
| `live_play` (test profile, demo)                         | 1.21.1: started 3.8 s, resources loaded 14.3 s, `stopped`; 26.3 (Vulkan, lavapipe): started 4.6 s, loaded 22.7 s, `stopped`. Profile name `TestPlayer` reached the client; the placeholder token is not in the run log                                                           |

## Demo play without an account (D-033)

- **Date / host:** 2026-10-17, development host (Ubuntu 24.04.5 container, Xvfb, Mesa
  llvmpipe), release core of branch `claude/demo-without-account`, not signed in.

| Check                          | Result                                                                                                                                                                                                         |
| ------------------------------ | -------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------- |
| `npm run check`                | Pass (58 frontend tests, 251 Rust unit tests and the integration tests)                                                                                                                                        |
| Home screen without an account | The ready installation offers **Play demo** with the demo explanation; no Play button                                                                                                                          |
| Play demo (1.21.1)             | Game window after 15 s; title screen reads "Minecraft 1.21.1 Demo" with "Play Demo World" ([png](verification/demo-release-title.png)); log: `Setting user: Player`; placeholder token absent from the run log |
| Ending the run                 | The headless X server stopped answering clients while the game ran, so the game was ended with SIGTERM; the run was recorded as failed with exit code 143, as expected                                         |

## Microsoft client ID (D-034, gate G0 in part)

- **Date / host:** 2026-10-17, development host; client ID `96c77e97-0788-4e3a-8eaf-91c8b143ba86`
  from the repository variable. Minecraft service access pending; no account signed in.

| Check                                   | Result                                                                                                                                                                                                                                     |
| --------------------------------------- | ------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------ |
| Package workflow (manual run on `main`) | [Run 39037754548](https://github.com/kurasis/launcher/actions/runs/39037754548) builds with `LAUNCHER_MSA_CLIENT_ID` set                                                                                                                   |
| Release core with the ID                | The ID is compiled in; Account shows **Sign in with Microsoft**                                                                                                                                                                            |
| Sign in                                 | Opens the system browser with the consumers authorize URL: the client ID, `redirect_uri=http://localhost:<port>/`, scopes `XboxLive.SignIn XboxLive.offline_access`, PKCE S256, `prompt=select_account`; UI shows "Signing in" with Cancel |
| That URL against Microsoft              | `login.microsoftonline.com` 302 → `login.live.com` 200 with the account sign-in form; the alternative scope set behaves the same                                                                                                           |
| Negative controls                       | The same request with an unregistered redirect or an unknown client ID ends on Microsoft's `invalid_request` "redirect_uri is not valid" page, so the positive result reflects this registration                                           |
| Loopback callback                       | A callback without the attempt's `state` is answered with 400; Cancel ends the attempt ("Sign-in was cancelled")                                                                                                                           |
| Not checked                             | Entering credentials, the token, Xbox, XSTS and Minecraft steps (owner-operated account test, after Minecraft service approval)                                                                                                            |

## Every vanilla version (D-035)

- **Date / host:** 2026-10-17, development host (Ubuntu 24.04.5 container, Xvfb with
  RANDR and `xrandr`, Mesa llvmpipe/lavapipe, no audio device), branch
  `claude/all-versions`.

### Planning every catalog version (`live_all_versions`)

919 versions (103 releases, 755 snapshots, 26 beta, 35 alpha) planned against the real
catalog, descriptors and asset indexes for four targets:

| Target                            | Result                                                                                                          |
| --------------------------------- | --------------------------------------------------------------------------------------------------------------- |
| Windows x64, Linux x64, macOS x64 | 919 of 919 plan (Java 8, 17, 21 or 25; modern, virtual and resources asset layouts; native jars where declared) |
| macOS arm64                       | 287 plan (every version without native classifier jars, 1.19 and later); 632 are refused as Intel-only          |

Failures found and fixed on the way: `os.version` rules need the OS version (the app
supplies it; the test now does), version IDs with spaces ("1.14 Pre-Release 5"), and
Mojang's older hosts `launcher.mojang.com` / `launchermeta.mojang.com` used by older
descriptors.

### Real installs and launches (Linux x64)

| Version           | Era / format                                  | Java | Install | Launch                                                                                       |
| ----------------- | --------------------------------------------- | ---- | ------- | -------------------------------------------------------------------------------------------- |
| `rd-132211`       | pre-Classic 2009, own main class              | 8    | Pass    | Pass (harness, no demo mode); world rendered                                                 |
| `c0.30_01c`       | Classic, launch wrapper                       | 8    | Pass    | Pass (harness); in game                                                                      |
| `a1.0.4`          | Alpha, `pre-1.6` resources                    | 8    | Pass    | Pass (harness); title screen                                                                 |
| `b1.7.3`          | Beta, `pre-1.6` resources                     | 8    | Pass    | Pass (harness); title screen. Demo identity refused (`NOTOKEN_UNAVAILABLE`, no Java started) |
| `1.5.2`           | launch wrapper, `pre-1.6` resources           | 8    | Pass    | Pass (harness); title screen with resources                                                  |
| `1.6.4`           | `minecraftArguments`, `virtual` legacy assets | 8    | Pass    | Pass, demo mode; `--session` redacted in the log                                             |
| `1.7.10`          | `minecraftArguments`, LWJGL 2 natives         | 8    | Pass    | Pass, demo mode                                                                              |
| `1.12.2`          | `minecraftArguments`, LWJGL 2 natives         | 8    | Pass    | Pass, demo mode, 22.7 s to the title screen                                                  |
| `1.16.5`          | modern arguments, LWJGL 3 native jars         | 8    | Pass    | Pass, demo mode                                                                              |
| `1.17.1`          | declares Java 16                              | 17   | Pass    | Pass, demo mode                                                                              |
| `26.4-snapshot-4` | current snapshot                              | 25   | Pass    | Pass, demo mode (first-start screen)                                                         |

All eleven share one cache: the Java 8 runtime was downloaded once (0.1 GiB) and reused
by nine versions. Screenshots: [all-versions-launch.jpg](verification/all-versions-launch.jpg);
the picker in a release build: [releases](verification/version-picker-releases.png),
[alpha](verification/version-picker-alpha.png).
"Harness" means `live_play` with `LAUNCHER_LIVE_FULL=1`, a test-only identity without
`--demo` for versions that have no demo mode; the product refuses a demo launch of them.

## Mojang Java runtimes and the Settings screen (D-036)

- **Date / host:** 2026-10-17, same development host, branch `claude/settings-and-java`.

### Automated

- Unit tests: index selection per platform (named component, same-major substitute,
  none on Linux ARM and for Java 8 on Apple silicon), version-name majors, unusable
  index entries, the real `jre-legacy` Linux manifest (300 files, 3 links), manifest
  checksum, unsafe paths, foreign hosts and size limits.
- Installer against the local fixture server: a manifest-built runtime installs, keeps
  its executable bits and links, downloads identical files once, removes its downloads,
  is reused without network, and is rebuilt after damage; wrong file bytes, an escaping
  link or a missing `java` fail before anything runs.
- Every current Mojang manifest (29: Linux, Windows x64/ARM, both Macs, every component)
  satisfies the extractor's path and link rules; Windows manifests have no links.

### Live (`live_runtime`)

| Component              | Release    | Files / bytes downloaded | Probe                         |
| ---------------------- | ---------- | ------------------------ | ----------------------------- |
| `jre-legacy`           | 8u202      | 262 / 235 MB             | `1.8.0_202`, spec 8, Oracle   |
| `java-runtime-alpha`   | 16.0.1.9.1 | 133 / 105 MB             | `16.0.1`, spec 16, Microsoft  |
| `java-runtime-gamma`   | 17.0.15    | 132 / 100 MB             | `17.0.15`, spec 17, Microsoft |
| `java-runtime-delta`   | 21.0.7     | 135 / 109 MB             | `21.0.7`, spec 21, Microsoft  |
| `java-runtime-epsilon` | 25.0.1     | 146 / 116 MB             | `25.0.1`, spec 25, Microsoft  |

Each was reused on a second request. Manifests resolve for Windows x64 (all five
components), Windows ARM and Apple silicon (17, 21, 25; `-alpha`/`-beta` map to
`-gamma`; Java 8 falls back to Temurin), Intel macOS (all five, Java at
`jre.bundle/Contents/Home/bin/java`) and none for Linux ARM (Temurin).

### Real installs and launches on Mojang runtimes (Linux x64)

A fresh data directory with the default setting:

| Version           | Component              | Java           | Install                 | Launch          |
| ----------------- | ---------------------- | -------------- | ----------------------- | --------------- |
| `b1.7.3`          | `jre-legacy` (default) | Mojang 8u202   | Pass, runtime 262 files | Pass (harness)  |
| `1.12.2`          | `jre-legacy`           | Mojang 8u202   | Pass, runtime reused    | Pass, demo mode |
| `1.17.1`          | `java-runtime-alpha`   | Mojang 16.0.1  | Pass                    | Pass, demo mode |
| `1.20.1`          | `java-runtime-gamma`   | Mojang 17.0.15 | Pass                    | Pass, demo mode |
| `26.4-snapshot-4` | `java-runtime-epsilon` | Mojang 25.0.1  | Pass                    | Pass, demo mode |

That data directory has no Temurin runtime at all. Then:

- **Repair:** one file of the Java 16 runtime overwritten → Repair downloaded the
  runtime's 133 files again and rebuilt a tree with the locked receipt hash; Pass.
- **Switch:** `javaProvider` set to Temurin → a new 1.17.1 installation got Temurin
  `jdk-17.0.20.1+1`; the existing 1.17.1 kept Mojang 16. Pass.
- **Older data:** four installations made with Temurin before this change (locks and
  runtime receipts without a provider field) still verify and launch (1.12.2, 1.17.1,
  26.4-snapshot-4, b1.7.3). Pass.

### Settings screen (release build)

Release build against the data directory above: the Java section lists each
installation's runtime from its lock, and choosing a source writes `javaProvider` to
`settings.json` (toggled to Temurin and back by clicking). Screenshots:
[Java](verification/settings-java.png), [Installations](verification/settings-installations.png).

## Console window (D-037)

- **Date / host:** 2026-10-17, same development host, release build, branch
  `claude/debug-console`.
- **Automated:**
  - journal paging, dropped-entry counts, long-line cuts and log4j folding (unit);
  - a launch whose fake game prints its access token: the journal has the
    redacted arguments (`--accessToken <redacted>`) and game line, and never the
    token;
  - the console capability grants only `allow-read-console`
    (`security_config`);
  - the window's polling, source/level/text filters, clear and the core-unreachable
    notice (Vitest).
- **Live (Xvfb, real game):**
  - Ctrl+Shift+L and **Diagnostics → Open console** both open the window, and a
    second request focuses it.
  - Minecraft 1.12.2 demo started from the app with the console open. The main
    window unloaded while the console kept updating with the game's lines, folded
    from 132 XML lines to 70 entries with warnings highlighted. After **Quit Game**
    the main window returned and the console recorded `exited, exit code 0`.
  - The Launcher filter shows the Java path and every argument with the token
    redacted.
  - Closing the main window with the close button (WM_DELETE_WINDOW) and no game
    running closed the console and ended the process.
  - Screenshots: [game output](verification/console-game.jpg),
    [launcher journal](verification/console-launcher.jpg).

## Interface languages (D-038)

- **Date / host:** 2026-10-17, release build, branch `claude/i18n`.
- **Automated:**
  - `npm run i18n:check`: 3 catalogs parse as ICU; English has every one of the 194
    user-facing Rust texts; no stale keys or foreign values; generated files current.
  - Rust: language tags are checked for form, a malformed tag in `settings.json`
    means English; templated errors serialize their template and values and read
    back; storage notices carry templates.
  - Vitest:
    - English by default; a language loads when chosen;
    - `ru-RU` falls back to `ru`, `pt-BR` to English;
    - Arabic sets `dir="rtl"`;
    - Russian and Arabic plural forms (Arabic: zero, one, two, few, many, other);
    - number formats;
    - core messages translated by template, including a translated value;
    - an unknown message falls back to its code's sentence;
    - the language picker saves the tag.
  - Build: Russian and Arabic are separate files (`ru-*.js` 59 kB, `ar-*.js` 53 kB);
    the main bundle has English only.
- **Live (Xvfb, release build):**
  - Russian applied at start from the setting.
  - Choosing العربية in the picker wrote `"language": "ar"` and mirrored the whole
    interface: navigation, settings sections, radio buttons, cards and the hero scene.
    Paths stayed left-to-right.
  - The console window's toolbar switched to Arabic and right-to-left, its journal
    stayed left-to-right, and its title became «وحدة تحكم المشغّل».
  - Fixed on the way: the hero scene covered the title in right-to-left, and mixed
    "4096 MiB · 1280×720" values reordered (now isolated).
  - Screenshots: [settings](verification/ar-settings.png), [home](verification/ar-home.png),
    [console](verification/ar-console.png).

## Mod loaders (D-039)

- **Date / host:** 2026-10-17, Linux x64 container, Xvfb, branch `claude/mod-loaders`.
  Evidence label: **test profile, demo** (no account).
- **Automated:** Maven coordinates and paths (escapes refused), version ordering,
  NeoForge → game version mapping, the Forge installer-format cut-off, installer
  locations, processor argument filling, the Maven library layout of the 1.21.1
  fixture, and the profile merge (loader libraries first, the game's gson replaced,
  client named after the profile, arguments appended, a wrong game version, a second
  loader, a host off the lists and installer-only files without an installer
  refused). Vitest: the form lists only the loader's game versions, preselects the
  recommended loader version, marks betas, moves to a supported game version, and
  sends the loader with _Estimate_ and _Create_.
- **Live installs** (`live_install` with `LAUNCHER_LIVE_LOADER`, real sources, one data
  directory, recommended loader versions):

  | Game   | Loader                   | Versions listed | Installed                                   | Installer step                                                  |
  | ------ | ------------------------ | --------------- | ------------------------------------------- | --------------------------------------------------------------- |
  | 1.21.1 | Fabric 0.19.5            | 254             | succeeded                                   | —                                                               |
  | 1.21.1 | Quilt 0.30.1             | 307             | succeeded, 9.5 s                            | —                                                               |
  | 1.21.1 | Forge 52.1.0             | 69              | succeeded, 28.2 s                           | 3 client processors (sanitized mappings downloaded by the tool) |
  | 1.21.1 | NeoForge 21.1.262        | 257             | succeeded, 27.6 s                           | 6 processors, mappings from the verified download               |
  | 1.20.1 | Forge 47.4.10            | 137             | succeeded, 37.8 s                           | 6 processors, 7 files recorded                                  |
  | 1.20.1 | NeoForge 1.20.1-47.1.106 | 62              | succeeded, 21.6 s                           | as Forge 1.20.1                                                 |
  | 1.20.1 | Fabric 0.19.5            | 254             | succeeded, 8.0 s                            | —                                                               |
  | 1.12.2 | Forge 14.23.5.2859       | 13              | succeeded, 29.7 s                           | universal jar unpacked from the installer                       |
  | 26.3   | NeoForge 26.3.0.88       | 85              | succeeded, 62.4 s (with Java 25 and assets) | 1 processor                                                     |

- **Live launches** (`live_play` with `LAUNCHER_LIVE_LOADER`, demo): all nine reached
  the title screen in demo mode in 23–44 s and stopped cleanly. The logs name the
  loader ("Loading Minecraft 1.21.1 with Fabric Loader 0.19.5", "… with Quilt Loader
  0.30.1", "ModLauncher running … 1.21.1-forge-52.1.0 / neoforge-21.1.262"); the
  title screens show "Forge 52.1.0 (2 mods loaded)", "NeoForge 21.1.262 (2 mods)",
  "Minecraft 1.21.1 Demo (Modded)" ([title screens](verification/loaders-menus.png)).
  - **Found and fixed:** Forge 1.12.2 runs through Mojang's launchwrapper, whose Forge
    tweaker drops `--demo`: the game opened the full title screen (Singleplayer,
    Multiplayer). Demo launches of launchwrapper loaders are now refused
    (`NOTOKEN_UNAVAILABLE`, checked live); with an account they start as before.
  - NeoForge 26.3's early loading window asks for an OpenGL visual this Xvfb does not
    offer ("Couldn't find matching GLX visual"); with `earlyWindowControl = false` in
    the installation's `config/fml.toml` it started normally. Vanilla 26.3 needs no
    such change here. Not a launcher defect; real displays provide the visual.
- **Repair:** after deleting the patched `forge-1.21.1-52.1.0-client.jar` and
  corrupting the remapped client, launch refused with `REPAIR_REQUIRED` before
  starting Java; Repair skipped the processor whose output was intact, re-ran the
  other two, and produced the same patched jar (SHA-1 `2d2537d3…`, the hash Forge's
  install profile declares).
- **Release build through the interface:** the installation list shows
  "Minecraft 1.21.1 · Fabric 0.19.5" etc. ([installations](verification/loaders-installations.png));
  **Play demo** on Fabric started the game, which reported "Demo (Modded)", and the
  launcher returned after Quit Game ("exited normally"). In **New installation**,
  choosing Forge limited the releases to 44, choosing 1.21.1 listed its Forge builds
  with 52.1.0 marked recommended ([form](verification/loaders-form.png)); _Estimate_
  showed 92 libraries and 28.6 MiB still to download; _Create_ and _Install_ of
  Forge 52.0.57 showed the phase "Running the mod loader installer" and finished
  ("Downloaded 9 files (28.6 MiB)").
- **Not verified:** Windows and macOS installs of loaders (CI builds and unit tests
  only), launches with a signed-in account, mods themselves.

## Installation notes (D-041)

- **Automated:** `validate_notes` (line endings, control characters, the 4000-character
  limit, non-Latin text), the `set_instance_notes` state test (saved while the
  installation is busy, refused with control characters), and the interface test of
  the notes editor (`InstanceDetails.test.tsx`: shows, edits and saves notes; the list
  shows the first line).
- **Not verified live:** the notes editor was not exercised in the release build.

## Closing the launcher once the game opens (D-042)

- **Automated:** `launch::logs::shows_game_window` against the ready markers, and the
  installation panel test: while a launch holds the installation it says "The game is
  running." and offers no Cancel.
- **Release build, Linux x64 (Xvfb), no-token demo of 1.21.1:** with _Close the
  launcher once the game window opens_ selected, the launcher quit about 12 s after
  **Play demo**; the game kept running and entered the demo world. A launcher started
  again showed "Running: Minecraft" with **Stop game**, which ended the game.
- **Not verified:** Windows and macOS (CI builds only).
