// Frontend mirror of the Rust DTOs in src-tauri/src/state/dto.rs, src-tauri/src/error.rs
// and src-tauri/src/commands/. Field names use camelCase because the Rust structs are
// serialized with `rename_all = "camelCase"`. Keep both sides in sync and bump
// API_SCHEMA_VERSION on incompatible changes.

export const API_SCHEMA_VERSION = 1;

/** Whether a project-owned Microsoft public-client ID was supplied at build time. */
export type MicrosoftClientIdStatus = 'not_configured' | 'invalid' | 'configured_unverified';

export type BuildProfile = 'debug' | 'release';

/** Non-secret facts about the running Rust core, returned by `get_app_info`. */
export interface AppInfo {
  productName: string;
  appVersion: string;
  tauriVersion: string;
  os: string;
  arch: string;
  buildProfile: BuildProfile;
  microsoftClientId: MicrosoftClientIdStatus;
}

export type ErrorCode =
  | 'AUTH_CANCELLED'
  | 'AUTH_TIMEOUT'
  | 'AUTH_STATE_MISMATCH'
  | 'AUTH_REAUTH_REQUIRED'
  | 'AUTH_APP_NOT_APPROVED'
  | 'AUTH_ACCOUNT_RESTRICTED'
  | 'AUTH_FORBIDDEN'
  | 'AUTH_PROTOCOL_ERROR'
  | 'AUTH_NOT_CONFIGURED'
  | 'NO_GAME_ACCESS'
  | 'PROFILE_SETUP_REQUIRED'
  | 'SERVICE_UNAVAILABLE'
  | 'SECRET_STORE_UNAVAILABLE'
  | 'SECRET_STORE_DELETE_FAILED'
  | 'UNSUPPORTED_PLATFORM'
  | 'UNSUPPORTED_VERSION_METADATA'
  | 'UNSUPPORTED_SCHEMA'
  | 'UNSUPPORTED_PATH_ENCODING'
  | 'UNSUPPORTED_DATA_PATH'
  | 'NETWORK_ERROR'
  | 'RATE_LIMITED'
  | 'DISK_FULL'
  | 'PERMISSION_DENIED'
  | 'HASH_MISMATCH'
  | 'METADATA_CONFLICT'
  | 'UNSAFE_ARCHIVE'
  | 'RUNTIME_UNAVAILABLE'
  | 'RUNTIME_MISMATCH'
  | 'INSTANCE_BUSY'
  | 'INSTANCE_NOT_FOUND'
  | 'GAME_ALREADY_RUNNING'
  | 'GAME_SPAWN_FAILED'
  | 'GAME_EXITED_WITH_ERROR'
  | 'OPERATION_CANCELLED'
  | 'OPERATION_INTERRUPTED'
  | 'OPERATION_NOT_FOUND'
  | 'APP_BUSY'
  | 'REPAIR_REQUIRED'
  | 'REQUEST_ID_CONFLICT'
  | 'INVALID_INPUT'
  | 'INSUFFICIENT_MEMORY'
  | 'DATA_ROOT_LOCKED'
  | 'STORAGE_UNAVAILABLE'
  | 'STORAGE_ERROR'
  | 'INTERNAL';

export interface LauncherError {
  code: ErrorCode;
  message: string;
  retryable: boolean;
  operationId?: string;
  diagnosticId: string;
  safeDetails?: Record<string, string | number | boolean>;
}

export type Theme = 'system' | 'light' | 'dark';
export type DuringGameMode = 'unload_and_return' | 'keep_open';
export type InstallationHealth =
  'not_installed' | 'ready' | 'needs_verification' | 'repair_required' | 'unsupported';
export type OperationKind = 'install' | 'repair' | 'launch';
export type Outcome = 'succeeded' | 'failed' | 'cancelled' | 'interrupted';

export type AccountState =
  | 'signed_out'
  | 'signing_in'
  | 'refreshing'
  | 'verified_access'
  | 'no_access'
  | 'profile_setup_required'
  | 'reauth_required'
  | 'service_unavailable'
  | 'app_not_approved'
  | 'restricted'
  | 'protocol_error'
  | 'sign_out_pending';

export interface Resolution {
  width: number;
  height: number;
}

export interface LastResult {
  kind: OperationKind;
  outcome: Outcome;
  finishedAt: string;
  errorCode?: ErrorCode;
  exitCode?: number;
}

export interface InstanceSummary {
  id: string;
  displayName: string;
  versionId: string;
  manifestSha1: string | null;
  createdAt: string;
  lastPlayedAt: string | null;
  maxHeapMib: number;
  resolution: Resolution;
  health: InstallationHealth;
  lastResult: LastResult | null;
  busy: boolean;
}

export interface SettingsView {
  theme: Theme;
  duringGameMode: DuringGameMode;
  selectedInstanceId: string | null;
  defaultMaxHeapMib: number | null;
  defaultResolution: Resolution;
  writable: boolean;
}

export interface AccountSummary {
  state: AccountState;
  profileName: string | null;
  profileId: string | null;
}

export interface DataRootInfo {
  status: 'ready' | 'unavailable';
  path: string | null;
  error: LauncherError | null;
}

export interface MemoryPolicy {
  physicalMib: number;
  minHeapMib: number;
  maxHeapMib: number;
  recommendedHeapMib: number;
  lowMemory: boolean;
}

export interface ResolutionBounds {
  minWidth: number;
  maxWidth: number;
  minHeight: number;
  maxHeight: number;
}

export interface MemoryLimits {
  policy: MemoryPolicy | null;
  error: LauncherError | null;
  resolution: ResolutionBounds;
}

export interface SupportedVersion {
  id: string;
  status: 'pending_verification' | 'qualified';
  expectedJavaMajor: number;
  acceptedDescriptorSha1: string[];
}

export interface ActiveActivity {
  kind: OperationKind;
  instanceId: string | null;
  operationId: string;
}

export type RunState =
  'preflight' | 'spawning' | 'running' | 'stopping' | 'exited' | 'failed' | 'cancelled';

export type RunKind = 'lifecycle_test' | 'game';

export interface RunSummary {
  runId: string;
  kind: RunKind;
  instanceId: string | null;
  state: RunState;
  startedAt: string;
  endedAt: string | null;
  exitCode: number | null;
  durationMs: number | null;
  drainTimedOut: boolean;
  outputTail: string[];
  outputLinesDropped: number;
  error: LauncherError | null;
}

export type LifecycleScenario =
  'immediate_exit' | 'short' | 'long' | 'crash' | 'inherited_pipe' | 'noisy';

export type QuitAction = 'quit' | 'keep_in_background' | 'stop_and_quit';

export type QuitResult =
  | { status: 'quitting' }
  | { status: 'in_background' }
  | { status: 'choice_required'; operationId: string };

export interface StorageIssue {
  code: ErrorCode;
  message: string;
  document: string;
}

export interface AppSnapshot {
  apiSchemaVersion: number;
  sessionEpoch: string;
  revision: number;
  appVersion: string;
  dataRoot: DataRootInfo;
  settings: SettingsView;
  account: AccountSummary;
  microsoftClientId: MicrosoftClientIdStatus;
  instances: InstanceSummary[];
  activeOperation: ActiveActivity | null;
  currentRun: RunSummary | null;
  lastRun: RunSummary | null;
  lifecycleHarnessAvailable: boolean;
  supportedVersions: SupportedVersion[];
  memory: MemoryLimits;
  storageIssues: StorageIssue[];
}

/** Envelope for every mutating command. */
export interface MutationRequest<T> {
  requestId: string;
  payload: T;
}

export interface CreateInstancePayload {
  displayName: string;
  versionId: string;
}

export interface UpdateInstanceSettingsPayload {
  instanceId: string;
  maxHeapMib: number;
  resolution: Resolution;
}

export interface SettingsPatch {
  theme?: Theme;
  duringGameMode?: DuringGameMode;
  selectedInstanceId?: string;
  defaultMaxHeapMib?: number;
  defaultResolution?: Resolution;
}

interface EventEnvelope {
  sessionEpoch: string;
  revision: number;
  timestamp: string;
}

export interface InstanceChangedEvent extends EventEnvelope {
  instance: InstanceSummary;
}

export interface SettingsChangedEvent extends EventEnvelope {
  settings: SettingsView;
}

export interface GameStateChangedEvent extends EventEnvelope {
  currentRun: RunSummary | null;
  lastRun: RunSummary | null;
  activeOperation: ActiveActivity | null;
}

export type BackendEvent =
  | { name: 'instance_changed'; payload: InstanceChangedEvent }
  | { name: 'settings_changed'; payload: SettingsChangedEvent }
  | { name: 'game_state_changed'; payload: GameStateChangedEvent };

export type BackendEventName = BackendEvent['name'];

export const BACKEND_EVENT_NAMES: readonly BackendEventName[] = [
  'instance_changed',
  'settings_changed',
  'game_state_changed',
];
