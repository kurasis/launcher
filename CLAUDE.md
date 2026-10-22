# Notes for working on this repository

## Windows (the app's windows, on every OS)

Every window is made through `lifecycle::queue_on_main_thread`, never with
`run_on_main_thread` or `WebviewWindowBuilder::build()` called directly from a command, a
window event or a plugin callback.

These run on the main thread: synchronous commands (`#[tauri::command] fn`, not
`async fn`), window and menu events, and plugin callbacks. There `run_on_main_thread`
runs the task inline instead of queueing it. A window built inline waits on WebView2
inside its own callback: on Windows it opens empty and the launcher hangs. Linux and macOS
do not show it, so CI and a Linux run will not catch it. This happened to the console
window (D-037) and to the launcher account window (D-064).

- A new window goes through `queue_on_main_thread` (see `show_console_window`), and its
  file is added to `windows_are_made_through_the_queue` in
  `src-tauri/tests/security_config.rs`, which fails otherwise.
- Commands that open a native dialog (`blocking_pick_file`, `blocking_save_file`, …) are
  `async fn`.
- Test any window change on Windows: a package from the Package workflow (`windows-x64`
  or `windows-arm64` artifact).

## Standing rules

- Security first: validate and bound everything from forms, the network and files; test
  injection and junk input.
- The repository and its workflow logs are public. Nothing about players or the server
  (addresses, nicks, emails, IPs, server text) goes into logs, issues or pull requests.
  Server tasks in workflows print fixed words and numbers only.
- The server is reached only through GitHub Actions (secrets `SSH_HOST`, `SSH_KEY`,
  `SSH_KNOWN_HOSTS`, `SSH_USER`). Never print a secret. Services live under
  `/home/www/lapi/<service>`, bind-mounted from the larger disk (docs/SERVER-STORAGE.md).
- `lapi` appears only as the domain (`*.lapi.sh`), never as a name in code.
- The update signing key never goes on the server.
- Crash reports are sent only when the player presses send. Anonymous statistics without
  IDs (D-071) are on by default and turned off in Settings → Privacy, which stops sending
  and erases what waits; nothing else turns them back on. Their numbers are read only on
  the server (docs/STATS.md), never in workflow logs.
- After editing the UI language catalogs, run `npm run i18n`.
- `OFFLINE_NEEDS_OWNER` (`src-tauri/src/state/mod.rs`) is off only while Microsoft has
  not approved the launcher's sign-in and builds are not published. Before any public
  release it goes back to `true`: offline in the full game (no `--demo`) only with a
  Microsoft account that owns Minecraft signed in.
