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
  /** Debug builds only: the no-token test profile that authorizes launches. */
  testProfile: string | null;
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
  | 'NOTOKEN_UNAVAILABLE'
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
  | 'LOADER_INSTALL_FAILED'
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
  /** English template of `message` with `{name}` values (D-038). */
  template?: string;
  params?: Record<string, string | number | boolean>;
}

export type Theme = 'system' | 'light' | 'dark';
export type DuringGameMode = 'unload_and_return' | 'keep_open' | 'close_launcher';
/** Interface language as a BCP 47 tag, e.g. `en`, `ru`, `ar`, `pt-BR` (D-038). */
export type Language = string;
/** Where private Java runtimes come from (D-036). */
export type JavaProvider = 'mojang' | 'temurin';

/** The private Java runtime an installation uses. */
export interface InstalledJava {
  provider: JavaProvider;
  major: number;
  release: string;
}
export type InstallationHealth =
  'not_installed' | 'ready' | 'needs_verification' | 'repair_required' | 'unsupported';
export type OperationKind = 'install' | 'repair' | 'launch' | 'relocate' | 'backup' | 'restore';

/** How the data folder was chosen (D-043). */
export type LocationMode = 'default' | 'custom' | 'portable';

/** The data folder in use. */
export interface DataLocation {
  mode: LocationMode;
  path: string;
  defaultPath: string;
  /** Where portable mode keeps data, beside the launcher. */
  portablePath: string | null;
  /** A move made this session; used from the next start. */
  pendingPath: string | null;
  /** The pending folder already held launcher data and is used as it is. */
  pendingReused: boolean;
}

export type LocationTarget = 'default' | 'custom' | 'portable';
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
  java: InstalledJava | null;
  /** The mod loader on top of the game; null for vanilla (D-039). */
  loader: LoaderChoice | null;
  /** The user's own notes; empty when none. */
  notes: string;
  /** A provider the next repair switches the private runtime to (D-044). */
  javaProvider: JavaProvider | null;
  /** The user's own Java, used instead of the private runtime (D-044). */
  customJava: CustomJava | null;
  /** Extra JVM options as typed; empty when none. */
  jvmArguments: string;
  /** Who plays this installation; null plays as the default profile (D-045). */
  profile: ProfileRef | null;
}

/** Why a Java may not suit a game version (D-044). */
export type JavaIssue = 'older' | 'newer' | 'architecture';

export interface CustomJava {
  path: string;
  javaVersion: string;
  major: number;
  /** Compatibility warnings the user accepted. */
  accepted: JavaIssue[];
}

/** What a chosen Java reported, and how it suits the game. */
export interface JavaCheck {
  path: string;
  javaVersion: string;
  major: number;
  vendor: string | null;
  arch: string;
  requiredMajor: number;
  issues: JavaIssue[];
}

export type JavaChoice =
  { kind: 'managed'; provider: JavaProvider } | { kind: 'custom'; path: string };

export interface SetInstanceJavaPayload {
  instanceId: string;
  java: JavaChoice;
  jvmArguments: string;
  /** The user saw the warnings of a custom Java and chose it anyway. */
  acceptIssues: boolean;
}

/** Mod loaders the launcher installs (D-039). */
export type LoaderKind = 'fabric' | 'quilt' | 'forge' | 'neoforge';

/** A loader and its exact version. */
export interface LoaderChoice {
  kind: LoaderKind;
  /** The loader's own id, e.g. `0.16.14`, `1.21.1-52.1.0` (Forge) or `21.1.262`. */
  version: string;
}

/** One loader version for the picker, newest first. */
export interface LoaderVersion {
  id: string;
  /** What people call it, e.g. Forge `52.1.0`. */
  label: string;
  stable: boolean;
  recommended: boolean;
}

export interface SettingsView {
  theme: Theme;
  duringGameMode: DuringGameMode;
  selectedInstanceId: string | null;
  defaultMaxHeapMib: number | null;
  defaultResolution: Resolution;
  javaProvider: JavaProvider;
  language: Language;
  /** Back up the worlds a game session changed, after it ends (D-047). */
  backupAfterPlay: boolean;
  /** Automatic backups kept per world. */
  backupKeep: number;
  writable: boolean;
}

/** Who plays: a Microsoft account (Minecraft profile ID) or a no-token profile (D-045). */
export type ProfileRef = { kind: 'microsoft'; id: string } | { kind: 'notoken'; id: string };

/** One Microsoft account. Never contains tokens. */
export interface MicrosoftAccount {
  profileId: string;
  profileName: string;
  state: AccountState;
  error: LauncherError | null;
}

/** An account whose saved credential could not be deleted yet. */
export interface PendingSignOut {
  profileId: string;
  profileName: string;
}

/** A local player name for Minecraft's demo. */
export interface NoTokenProfile {
  id: string;
  name: string;
}

/** Accounts and profiles (D-045). */
export interface AccountSummary {
  accounts: MicrosoftAccount[];
  /** The active browser sign-in, cancellable with `cancel_login`. */
  loginOperationId: string | null;
  /** `signing_in`, the state of the last failed sign-in, or `signed_out`. */
  loginState: AccountState;
  loginError: LauncherError | null;
  /** Credentials are kept in memory only (no usable OS vault). */
  sessionOnly: boolean;
  pendingSignOuts: PendingSignOut[];
  notokenProfiles: NoTokenProfile[];
  /** Who installations play as unless they name another profile. */
  defaultProfile: ProfileRef | null;
}

export type ExternalLink =
  | 'buy_minecraft'
  | 'create_profile'
  | 'xbox_profile'
  | 'family_settings'
  | 'minecraft_help'
  | 'minecraft_eula'
  | 'source_code';

/** Result of `export_diagnostics`. */
export interface ExportResult {
  /** False when the save dialog was closed. */
  saved: boolean;
  fileName: string | null;
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

/** Mirrors `catalog::source::Availability`. */
export type VersionAvailability = 'available' | 'not_in_catalog' | 'catalog_unavailable';

/** Mojang's vanilla version types (D-035). */
export type VersionType = 'release' | 'snapshot' | 'old_beta' | 'old_alpha';

export interface VersionView {
  id: string;
  versionType: VersionType;
  releaseTime: string | null;
  /** Shipped, pinned and checked by this project. */
  verified: boolean;
  availability: VersionAvailability;
}

/** Returned by `list_supported_versions`: every vanilla version, newest first.
 * `latestRelease` and `latestSnapshot` are informational only. */
export interface CatalogView {
  versions: VersionView[];
  fetchedAt: string | null;
  stale: boolean;
  error: LauncherError | null;
  latestRelease: string | null;
  latestSnapshot: string | null;
}

/** Returned by `preview_install_plan`; nothing is downloaded to produce it. */
export interface PlanSummary {
  versionId: string;
  descriptorSha1: string;
  javaMajor: number;
  libraries: number;
  assetObjects: number;
  gameArtifacts: number;
  gameBytes: number;
  missingGameBytes: number;
  /** Unknown until the Java runtime is resolved. */
  runtimeBytes: number | null;
}

export interface ActiveActivity {
  kind: OperationKind;
  instanceId: string | null;
  operationId: string;
}

export type RunState =
  'preflight' | 'spawning' | 'running' | 'stopping' | 'exited' | 'failed' | 'cancelled' | 'stopped';

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
  /** The run's output log could not be written; output was kept in memory. */
  loggingDegraded: boolean;
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
  template?: string;
  params?: Record<string, string>;
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
  /** Installs and repairs waiting their turn, first first. */
  queue: QueuedOperation[];
  currentRun: RunSummary | null;
  lastRun: RunSummary | null;
  lifecycleHarnessAvailable: boolean;
  supportedVersions: SupportedVersion[];
  memory: MemoryLimits;
  storageIssues: StorageIssue[];
  operationProgress: OperationProgress | null;
  recentOperations: OperationResult[];
}

/** Phase of a running install or repair (doc 06 `InstallState`). */
export type InstallState =
  | 'resolving'
  | 'checking_files'
  | 'downloading'
  | 'extracting_runtime'
  | 'preparing_loader'
  | 'finalizing'
  | 'cancel_requested';

/** Coalesced progress; does not advance the state revision. */
export interface OperationProgress {
  sessionEpoch: string;
  operationId: string;
  instanceId: string | null;
  kind: OperationKind;
  sequence: number;
  revision: number;
  phase: InstallState;
  downloadedBytes: number;
  totalDownloadBytes: number | null;
  verifiedFiles: number;
  totalFiles: number | null;
  bytesPerSecond: number | null;
  /** Names of the files being downloaded right now (D-049). */
  currentFiles: string[];
  timestamp: string;
}

/** An install or repair waiting its turn (D-049). */
export interface QueuedOperation {
  queueId: string;
  instanceId: string;
  kind: OperationKind;
  queuedAt: string;
}

/** The one terminal outcome of an install or repair. */
export interface OperationResult {
  operationId: string;
  instanceId: string | null;
  kind: OperationKind;
  outcome: Outcome;
  error: LauncherError | null;
  finishedAt: string;
  downloadedFiles: number;
  networkBytes: number;
  reusedFiles: number;
}

export interface OperationStarted {
  /** The operation ID, or the queue ID when `queued`. */
  operationId: string;
  queued?: boolean;
}

export interface CancelAck {
  /** `cancel_requested` while running, otherwise the recorded outcome. */
  state: string;
}

/** Envelope for every mutating command. */
export interface MutationRequest<T> {
  requestId: string;
  payload: T;
}

export interface CreateInstancePayload {
  displayName: string;
  versionId: string;
  loader?: LoaderChoice | null;
}

export interface SetInstanceNotesPayload {
  instanceId: string;
  notes: string;
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
  javaProvider?: JavaProvider;
  language?: Language;
  backupAfterPlay?: boolean;
  backupKeep?: number;
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
  queue: QueuedOperation[];
}

export interface AccountStateChangedEvent extends EventEnvelope {
  account: AccountSummary;
}

export interface OperationFinishedEvent extends EventEnvelope {
  result: OperationResult;
  activeOperation: ActiveActivity | null;
}

export type BackendEvent =
  | { name: 'instance_changed'; payload: InstanceChangedEvent }
  | { name: 'settings_changed'; payload: SettingsChangedEvent }
  | { name: 'game_state_changed'; payload: GameStateChangedEvent }
  | { name: 'operation_progress'; payload: OperationProgress }
  | { name: 'operation_finished'; payload: OperationFinishedEvent }
  | { name: 'account_state_changed'; payload: AccountStateChangedEvent };

export type BackendEventName = BackendEvent['name'];

export const BACKEND_EVENT_NAMES: readonly BackendEventName[] = [
  'instance_changed',
  'settings_changed',
  'game_state_changed',
  'account_state_changed',
  'operation_progress',
  'operation_finished',
];

/** One console journal entry (D-037). Game lines are already redacted. */
export interface ConsoleEntry {
  seq: number;
  at: string;
  source: 'launcher' | 'game';
  level: 'info' | 'warn' | 'error';
  stderr?: boolean;
  text: string;
}

export interface ConsolePage {
  entries: ConsoleEntry[];
  lastSeq: number;
  /** Entries dropped from the journal before this reader saw them. */
  missed: number;
}

/** What a backup holds (D-047). */
export type BackupScope = { kind: 'world'; name: string } | { kind: 'installation' };

export type BackupReason = 'manual' | 'after_play' | 'before_restore';

export interface BackupInfo {
  id: string;
  instanceId: string;
  instanceName: string;
  versionLabel: string;
  scope: BackupScope;
  reason: BackupReason;
  createdAt: string;
  fileCount: number;
  totalBytes: number;
  /** Bytes this backup added; files shared with other backups count once. */
  newBytes: number;
}

export interface BackupOverview {
  /** Newest first. */
  backups: BackupInfo[];
  worlds: { instanceId: string; worlds: string[] }[];
  storeBytes: number;
}
