# 06 — User interface and frontend/backend contracts

## UI direction

Create a restrained desktop web UI with a clear primary Play action, installation cards/list, persistent account status and a visible downloads area. Use original generic visual assets; do not borrow official Minecraft launcher artwork, logos or another launcher's design assets. A temporary neutral product name is sufficient during implementation.

Use CSS variables for color, spacing, radii, typography and motion. Separate visual components from business operations so changing themes/layouts does not affect auth or launch code. Package fonts locally or use a system font stack. No remote scripts, ads or news feed.

Baseline window 1100 × 720, minimum around 820 × 560, with usable scrolling at smaller effective sizes and 200% text scaling. Native window decorations are the MVP default; custom draggable title bars are optional after cross-platform verification. Buttons, dialogs and menus must support keyboard navigation, focus indication and accessible names. Honor reduced-motion and system theme preferences.

## Screens

| Screen | Required content and actions |
| --- | --- |
| Home / installations | Selected instance, exact game version, installation status, Play/Install, create instance, account summary |
| Create installation | Name and supported release; clear isolated-directory explanation; validation before creation |
| Installation details | RAM, resolution, open game folder, verify/repair, last-run result |
| Account | System-browser sign-in, cancel, state/reason, profile name and sign-out; no password fields |
| Downloads | Actual phase, aggregate bytes, completed/total files, retry and cancel; honest indeterminate preparation |
| Settings | Theme; during-game mode (`unload_and_return` or `keep_open`); memory/resolution defaults; read-only data location |
| Diagnostics | Sanitized recent logs, exit code, actionable error, explicit export action |
| About | Product version, independent-project notice, source/license information, official Minecraft links |

Switching instances must never silently change the version of an existing world. Launch, Install, Repair and settings controls reflect backend locks. A failed operation leaves a visible recovery action. UI-only optimistic state cannot enable Play against backend policy.

## Bootstrap and ownership

On every webview creation, subscribe to events and request `get_app_snapshot`. Use a backend `sessionEpoch` plus monotonic state `revision` to reconcile snapshots and events. Buffer events until the snapshot arrives, discard revisions already represented, apply newer state updates in order, and refetch on gaps. A changed epoch resets client caches; revisions from different backend processes are not comparable. The Rust state is authoritative; React may cache presentation state but cannot change run/install/account status itself.

Every accepted long operation returns an `operationId` promptly; rejected requests return a typed error. Mutating commands also take a generated `requestId`. Within one backend epoch, duplicate IDs with identical payloads return the same result/operation, and conflicting reuse is rejected. After restart reconcile the snapshot and durable job/run receipts before retrying; an IPC timeout is not proof that nothing happened. Events carry the same ID, an increasing sequence/revision, relevant instance/run ID and a timestamp. A rebuilt frontend obtains current operation state from the snapshot; it does not rely on receiving an old completion event.

## Commands

Arguments and responses must be typed on both sides. Validate in Rust even if TypeScript validates them. All mutating commands use `MutationRequest<T> { requestId, payload: T }`; the table lists payload fields only, with `None` meaning an empty payload. Read-only snapshot/catalog-without-refresh/log queries need no mutation ID. Catalog refresh is a coalesced source query, not a user-data mutation. Suggested contract names:

| Command | Input | Result |
| --- | --- | --- |
| `get_app_snapshot` | None | `AppSnapshot` |
| `list_supported_versions` | Optional refresh flag | Supported catalog entries and cache age |
| `create_instance` | Display name, exact supported version ID | Instance summary |
| `update_instance_settings` | Instance ID, validated RAM/resolution | Updated summary |
| `begin_login` | None | Operation ID; browser opened by Rust |
| `cancel_login` | Operation ID | Acknowledgement |
| `sign_out` | None | Clear launch authorization and remove credentials; cancel/join auth work, reject during owned launch/game, expose vault deletion failure |
| `install_instance` | Instance ID | Operation ID |
| `repair_instance` | Instance ID | Operation ID |
| `cancel_operation` | Operation ID | Cancellation requested |
| `launch_game` | Instance ID | Run/operation ID |
| `stop_game` | Run ID, confirmed termination intent | Stopping state |
| `get_log_tail` | Run ID, bounded limit/cursor | Sanitized log entries |
| `set_app_settings` | Validated allowed settings patch | Updated settings |
| `open_instance_folder` | Instance ID | Result; Rust resolves path |
| `export_diagnostics` | Optional run ID | Save dialog, redacted file export result |
| `show_main_window` | None | Window present/focused where OS permits |
| `request_quit` | Explicit quit intent | Quit or required user choice |

Do not expose arbitrary filesystem paths, unrestricted URL downloads, unrestricted shell execution, token getters, generic HTTP proxies or a Java-argument passthrough to the frontend.

## DTO examples

These shapes are a contract sketch to implement consistently; extend them through a versioned change, not ad hoc event fields.

```ts
type AccountState =
  | 'signed_out' | 'signing_in' | 'refreshing' | 'verified_access'
  | 'no_access' | 'profile_setup_required' | 'reauth_required'
  | 'service_unavailable' | 'app_not_approved' | 'restricted'
  | 'protocol_error' | 'sign_out_pending';

type InstallState =
  | 'not_installed' | 'resolving' | 'checking_files' | 'downloading'
  | 'extracting_runtime' | 'finalizing' | 'ready'
  | 'cancel_requested' | 'cancelled' | 'failed' | 'interrupted';

// Installation/repair progress only; auth/run events have their own state types.
interface OperationProgress {
  sessionEpoch: string;
  operationId: string;
  instanceId?: string;
  sequence: number;
  revision: number;
  phase: InstallState;
  downloadedBytes: number;
  totalDownloadBytes: number | null;
  verifiedFiles: number;
  totalFiles: number | null;
  bytesPerSecond: number | null;
  timestamp: string;
}

interface LauncherError {
  code: string;
  message: string;
  retryable: boolean;
  operationId?: string;
  diagnosticId: string;
  safeDetails?: Record<string, string | number | boolean>;
}
```

`AppSnapshot` includes API schema version, sessionEpoch, revision, app version, settings, account display/state, instance summaries, active operation, current run and last result. It never includes access/refresh tokens, OAuth parameters, secret-store contents or a token-bearing command line. Account state displayed as verified is informational; each launch still rechecks access.

Use consistent error envelopes for failed commands and operation-completion events. Dates are UTC ISO-8601 strings; byte counts are nonnegative and within JS safe integer bounds, or serialized as decimal strings if an implementation exceeds that range. Use generated identifiers, not arbitrary path strings supplied by React.

## Events

Names: `account_state_changed`, `operation_progress`, `operation_finished`, `instance_changed`, `game_state_changed`, `log_batch`, `settings_changed`.

The backend records one terminal transition per operation with success/cancel/error. Event delivery to a disposable webview is best-effort, not an exactly-once durable transport. Store terminal outcomes in receipts/snapshot history; consumers deduplicate by epoch/operation/sequence and reconcile after reconnect. An interrupted job after a backend crash is recorded as interrupted and offered recovery, never fabricated as completed. Progress can be coalesced; terminal outcomes remain queryable. Batch logs and progress updates to avoid flooding the webview. Unsubscribe on component disposal and do not register duplicate listeners across React development remounts.

## Stable error vocabulary

At minimum distinguish: `AUTH_CANCELLED`, `AUTH_TIMEOUT`, `AUTH_STATE_MISMATCH`, `AUTH_REAUTH_REQUIRED`, `AUTH_APP_NOT_APPROVED`, `AUTH_ACCOUNT_RESTRICTED`, `NO_GAME_ACCESS`, `PROFILE_SETUP_REQUIRED`, `SERVICE_UNAVAILABLE`, `SECRET_STORE_UNAVAILABLE`, `UNSUPPORTED_PLATFORM`, `UNSUPPORTED_VERSION_METADATA`, `NETWORK_ERROR`, `RATE_LIMITED`, `DISK_FULL`, `PERMISSION_DENIED`, `HASH_MISMATCH`, `UNSAFE_ARCHIVE`, `RUNTIME_UNAVAILABLE`, `RUNTIME_MISMATCH`, `INSTANCE_BUSY`, `GAME_ALREADY_RUNNING`, `GAME_SPAWN_FAILED`, `GAME_EXITED_WITH_ERROR`, `OPERATION_CANCELLED`, `OPERATION_INTERRUPTED`, `APP_BUSY`, `REPAIR_REQUIRED`, `AUTH_FORBIDDEN`, `AUTH_PROTOCOL_ERROR`, `SECRET_STORE_DELETE_FAILED`, `UNSUPPORTED_SCHEMA`, `UNSUPPORTED_PATH_ENCODING`, `UNSUPPORTED_DATA_PATH`, `METADATA_CONFLICT` and `REQUEST_ID_CONFLICT`.

User messages must describe the next action, not expose stack traces as the main UI. Keep structured non-secret context for diagnostics. Do not collapse account restrictions, app approval failures and missing licenses into one message.

## Snapshot and control details

Auth and game events use dedicated states (`RunState`: preflight, spawning, running, stopping, exited, failed, cancelled); do not put them into `InstallState`. Log batches have their own bounded cursor, not one state revision per line. Fetch missing logs through `get_log_tail`; display a truncation marker when old lines are no longer retained. Show active and recent terminal operations in snapshots so a recreated window can recover the outcome.

Map global `APP_BUSY` to a visible active-operation explanation even when another instance is selected. Disabling the UI is advisory; Rust enforces all locks. Defaults apply to new instances only. Existing instance settings are updated explicitly and cannot alter a running command.

P02 must specify concrete RAM/resolution bounds in a shared Rust validation module. Initial policy: heap 1024 MiB through `min(32768 MiB, physical RAM minus OS reserve)` with reserve `max(2048 MiB, 20% of physical RAM)`; round down to MiB and fail with a clear insufficient-memory message if the interval is empty. Initial heap is `min(512 MiB, maxHeapMiB)`. Resolution is integer width 640–7680 and height 480–4320. These are launcher input bounds, not promises of game performance; use a lower-capacity warning and qualify actual baseline hardware.

## Installation health is not operation progress

Represent persistent `InstallationHealth` separately as `not_installed | ready | needs_verification | repair_required | unsupported`. The InstallState sketch above describes an install/repair operation's phase/outcome; it is not the sole source of instance health. `AppSnapshot` exposes both. An interrupted first install is not installed; an interrupted repair is needs_verification/repair_required until the locked artifacts are checked. A cancelled repair can leave a valid historical lock without proving current health. A completed repair restores health only after verification and commit.

For `cancel_operation`, Rust distinguishes the operation kind: install/repair cooperatively cancels before commit; pre-spawn launch can cancel; an already running game requires `stop_game` and its explicit confirmation. An already terminal operation returns its recorded outcome. `request_quit` while login is active cancels it; while install/preflight/game is active it follows the state table in document 05.

Do not disable sign-out indefinitely because a background refresh happens to be running: cancel/join that refresh and invalidate its generation before the sign-out transaction, while still refusing sign-out during an owned launch/game. Expose a logging-degraded warning without blocking game supervision.
