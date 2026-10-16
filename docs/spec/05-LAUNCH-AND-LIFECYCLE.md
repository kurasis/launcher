# 05 — Launch command, game supervision and GUI lifecycle

## Launch preflight

`launch_game(instanceId)` is a Rust-owned operation, not a frontend shell command. It must acquire the global activity permit and instance lock, validate the instance and install lock, verify compatible platform/runtime and files, refresh credentials and current play access, create run directories and compile a launch plan. A second request must return the existing in-progress run or `GAME_ALREADY_RUNNING`; it must not spawn another Java process.

Block launch when installation is incomplete, the current metadata format is unsupported, required files are missing, runtime version/architecture mismatches, account access cannot be verified or the selected instance is already locked. If files need repair, offer it; do not fall through to Java and hope for the best.

## Argument compiler

Construct an ordered vector of arguments, never an interpolated shell command. Launch the absolute managed Java executable with the instance's `game/` as working directory. Preserve metadata JVM arguments, classpath position, `mainClass` and game arguments in their intended order.

Minimum conceptual structure: Java executable; selected memory settings; evaluated metadata JVM arguments; logging argument; declared main class; evaluated game arguments. If JVM properties conflict, use one documented deterministic value and test it. User-provided free-form JVM arguments are out of scope.

| Placeholder | Source |
| --- | --- |
| `auth_player_name` | Authenticated Java profile name |
| `auth_uuid` | Authenticated Java profile ID in the format required by the metadata |
| `auth_access_token` | Current Minecraft token, inserted in Rust only |
| `version_name` / `version_type` | Locked descriptor ID/type |
| `game_directory` | Absolute instance `game/` directory |
| `assets_root` / `assets_index_name` | Shared asset root / locked index ID |
| `natives_directory` | Absolute unique run-native root |
| `classpath` | Ordered applicable library artifact paths plus client JAR |
| `classpath_separator` | `;` on Windows and `:` on Unix, when referenced |
| `library_directory` | Not required by the two initial descriptors; reject a required occurrence until a compatible Maven-root adapter is implemented and tested |
| `launcher_name` / `launcher_version` | This launcher's own identifier/version |
| `resolution_width` / `resolution_height` | Validated instance settings |
| `user_type` | `msa` when the supported descriptor requests it |
| `clientid` | This project's verified Microsoft application ID |
| `auth_xuid` | Verified Xbox identifier if exposed by the chosen auth contract |
| `path` in logging argument | Verified logging-configuration path |

Do not synthesize an XUID from the Minecraft UUID. P04 records whether the accepted auth contract supplies an authenticated XUID and its exact source. If it does not, record empty-value handling as a launch compatibility candidate; P09 tests the first baseline and P13 tests both baselines, including authenticated server access. G0 does not require an earlier real-game test and cannot assert that empty is already compatible. Do not decode an unverified claim and call it authenticated. Unused feature-gated placeholders need not be resolved; every placeholder in an included argument must have an explicit implementation. An unresolved placeholder is a hard error.

Retain OS-specific metadata flags such as macOS first-thread behavior. Never replace the official JVM argument list with a small handwritten subset. Create suffix directories requested by newer native-path arguments before launch. Use the descriptor's logging configuration as declared; do not paste JVM fixes intended for unrelated older releases.

## Command length and secret-bearing arguments

Windows command-line length can be exceeded by a large classpath. Put the long non-secret JVM/classpath arguments in a restricted per-run argfile, followed by main class and game arguments as direct process arguments. Never write tokens to the argfile in MVP. Test Java 21/25 quoting, backslashes, comment markers, literal at-signs and non-ASCII paths. Argfiles follow Java launcher encoding rules; do not assume UTF-8 works on every Windows locale or that `-Dfile.encoding` changes argument parsing. Relative paths resolve from the process working directory. Test the actual runtime/locale with an argument-echo harness, then real Minecraft. If a required path cannot be represented, fail before spawn with `UNSUPPORTED_PATH_ENCODING`; do not corrupt it silently. Keep the argfile until process exit and remove it during cleanup. See the [Java launcher manual](https://docs.oracle.com/en/java/javase/25/docs/specs/man/java.html).

A human-readable launch preview must substitute `<redacted>` for secrets and shorten or anonymize home paths. The actual token may be observable to local same-user process tools when supplied to Java; do not claim this design eliminates that OS-level limitation.

## Process supervision

- Keep the child handle and run state in Rust, outside any window state.
- Read stdout and stderr concurrently so pipes cannot block the game.
- Apply streaming redaction before persisting or publishing launcher-captured logs; account for secrets split across input chunks. Bound line size and buffering.
- Bound launcher-captured output: 64 KiB maximum line payload with explicit truncation, 2,000-line/2 MiB in-memory tail, 10 MiB × 5 files per run. Retain at most 20 completed runs and 200 MiB of completed-run logs, pruning oldest first; active run files have their own rotation cap. Launcher service logs use a separate 10 MiB × 5 cap. These limits never delete game-owned logs or worlds. The streaming redactor must handle split secrets and end-of-stream tails without leaking retained fragments.
- Treat successful spawn as “process started,” not proof that Minecraft reached its main menu. A short survival interval can determine when to unload the GUI; it must not be labelled verified game readiness.
- If Java fails to spawn or exits immediately, keep/reopen GUI and show diagnostic context. Clean up the run lock and native staging even on failure.
- Await process exit without high-frequency polling. Record exit code or signal and duration. A nonzero code is a failure indication, not a complete diagnosis.
- “Stop game” is an explicit user action with an unsaved-world warning. Use supported termination semantics and wait before offering force termination. Windows termination may not provide graceful saving; do not label it “save and quit.” Kill only the owned process identity, never every `java` process.
- Normal player exit occurs from Minecraft. The launcher should not automatically terminate Java because its own window closed.

Game-owned logs/crash reports may contain sensitive values outside the launcher's direct control. Diagnostic export must re-sanitize all included material and exclude raw auth dumps, launch argfiles, account files, worlds and unrelated system files.

## Window and application states

| Event | Required behavior |
| --- | --- |
| Play accepted | Persist basic UI selection and begin Rust preflight |
| Java spawned and survives initial startup interval | In default mode, destroy the main `WebviewWindow`; retain supervisor |
| Destroying the last window triggers app exit request | Prevent automatic exit while an owned game/job requires supervision |
| User reopens the app / selects Show launcher | Single-instance handler creates or focuses exactly one window and restores state |
| Java exits normally | Persist result, drain output with bounds, clean run staging, release locks, ensure one window shows the selected installation |
| Java crashes | Same recreation path, plus an error card and redacted log tail |
| User closes GUI while game runs | Continue background supervision; do not terminate game |
| User closes GUI while idle | Exit application normally after bounded state flush |
| User closes GUI during browser login | Cancel the owned login attempt/listener, invalidate late callbacks, then exit; browser session is not remotely signed out |
| User closes GUI during installation | Offer keep installing in background or cancel-and-exit; keep the supervisor until the job terminates |
| Background install/repair finishes or fails | Persist the result and ensure one window is shown with the result; skip restoration only for a committed cancel-and-exit intent |
| User closes GUI during launch preflight | Cancel pre-spawn work and exit, or if spawn has committed use the running-game close policy; serialize this race |
| User chooses Quit while game runs | Offer Keep launcher in background (default) or Stop game and quit with unsaved-world warning |
| OS shutdown/session end | Best-effort flush; do not indefinitely prevent OS shutdown |

MVP does not offer deliberate detach-and-quit. Confirmed Stop game and quit waits for owned-child exit and bounded cleanup before exiting the launcher. A cancelled confirmation keeps supervision running. Unexpected supervisor crashes are handled by the separate orphan-recovery policy.

Use `RunEvent::ExitRequested` and the actual Tauri 2 APIs, with an explicit exit reason/intent flag so preventing last-window exit does not trap every deliberate quit. Serialize create/destroy operations on the app event loop. A game that exits while its window is being destroyed must still restore exactly one window. Recreated webviews receive the same restrictive capabilities and load the same packaged URL.

References: [Tauri Rust lifecycle](https://docs.rs/tauri/latest/tauri/enum.RunEvent.html), [WebviewWindow](https://v2.tauri.app/reference/javascript/api/namespacewebviewwindow/), [single-instance plugin](https://v2.tauri.app/plugin/single-instance/).

## Background access and recovery

A tray menu may provide Show launcher and Quit on systems that support it. Linux tray availability is not guaranteed, so opening the launcher executable again must restore the existing app without creating a second supervisor. macOS Dock/reopen must work similarly. Background operation must not depend solely on a tray extension.

While the UI is absent, stop UI subscriptions, animations and frontend polling; retain only required Rust tasks and bounded buffers. Destroying a webview releases its owned UI resources but is not a guarantee that every system WebView helper process disappears immediately. Measure the actual process tree instead of advertising a fixed memory footprint.

After unexpected launcher restart, detect an existing game using the persisted PID plus process start identity. Block duplicate launch, explain that monitoring was interrupted, and wait or allow a user-directed recovery. Full adoption of a previously orphaned process is optional; protecting its worlds from duplicate launch is mandatory.

## Performance goals

On documented reference machines with release builds and warm local storage, target an interactive launcher within 3 seconds, restored UI target within 2 seconds after child exit when output drains normally; allow up to 5 seconds for the bounded-drain recovery path, idle/background CPU near zero (target below 1% of one core averaged over a minute), and no sustained memory growth across 20 play/exit cycles. Memory must be measured for the complete launcher/WebView process tree separately from Java; report OS, method and baseline. These are qualification targets, not observed measurements or universal guarantees.

## Deterministic child environment and shutdown

For runtime probes and game launch, remove inherited `JDK_JAVA_OPTIONS`, `JAVA_TOOL_OPTIONS`, `_JAVA_OPTIONS` and `CLASSPATH` so external settings cannot add JVM flags or agents. Do not clear the entire environment: preserve required desktop/session, locale and graphics variables. Use absolute Java paths and Rust's argument API; never execute a shell wrapper. Record only the names of filtered variables, never their potentially secret values.

Do not infer quit intent solely from `ExitRequested.code`: menu/OS close paths vary. Route application menu Quit, keyboard Quit and tray Quit through the same explicit intent policy. Only invoke programmatic exit after that decision and cleanup. Pin and test the selected Tauri version's exit APIs; `prevent_exit` is not a universal cancellation mechanism for programmatic restart. Automatic restart is outside MVP. OS shutdown gets bounded best-effort cleanup without a keepalive loop.

Window creation/destruction runs on the main event loop. After a child exits, keep an existing open window or create one if absent; never create a duplicate in keep-open mode. In both UI modes publish the exit result. A delayed destroy request must be invalidated if the run already ended. If window restoration fails, persist the result and allow single-instance reopen to retry.

Drain stdout/stderr concurrently. After child exit allow at most two seconds for remaining pipe data, then close capture with a diagnostic if inherited handles prevent EOF; do not hang return forever. Retain redaction state through the final drain. Explicit child-handle lifetime policy must prevent window destruction from dropping or killing the supervised process; test early errors and cancellation separately.

## Readiness, cancellation and path boundaries

Serialize the final pre-spawn commit with cancel/account-generation checks. Before this boundary, cancellation prevents process creation. After successful spawn, generic operation cancellation must not silently kill Java; return the current run and require the explicit Stop game action. Record the run even if the frontend disconnected before receiving the launch reply.

Check DATA_ROOT and all generated paths before a large install. Java classpath separators are `;` on Windows and `:` on Unix; quotes around the process argument do not make a separator embedded in one classpath entry unambiguous. Reject unsupported roots containing the platform classpath delimiter with `UNSUPPORTED_DATA_PATH` and an actionable explanation before downloads. This is a documented MVP path constraint, not permission to erase or silently move user data. Recheck path/encoding/length compatibility before launch. Unicode itself is not prohibited; verify actual supported combinations under A16.

Disk-full or permission failure in the launcher log sink must not stop draining Java output. Switch to a bounded in-memory tail/discard policy with a visible logging-degraded diagnostic; never block the pipe task on repeated failed writes. Never store raw output as a fallback. A failing state flush must retain an honest recovery/error outcome, not silently claim successful persistence.
