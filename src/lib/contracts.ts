// Frontend mirror of the Rust types the core sends: most live in
// src-tauri/src/state/dto.rs and error.rs, the rest beside their feature (update.rs,
// friends.rs, traffic.rs, gallery/, launch/crash.rs, storage/models.rs, ...). Field
// names use camelCase because the Rust structs are serialized with
// `rename_all = "camelCase"`. Keep both sides in sync and bump API_SCHEMA_VERSION here
// and in dto.rs on incompatible changes; the two ship in one binary, so it records
// the version rather than being checked at run time.

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
  /** Extra Minecraft arguments as typed; empty when none (D-056). */
  gameArguments: string;
  /** Starts the game in fullscreen (D-056). */
  fullscreen: boolean;
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

/** Another version of the installation's mod loader (D-054). */
export interface SetInstanceLoaderPayload {
  instanceId: string;
  version: string;
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
  /** New installations start the game in fullscreen (D-056). */
  defaultFullscreen: boolean;
  javaProvider: JavaProvider;
  language: Language;
  /** Back up the worlds a game session changed, after it ends (D-047). */
  backupAfterPlay: boolean;
  /** Automatic backups kept per world. */
  backupKeep: number;
  /** The connection is metered: big downloads ask first (D-051). */
  metered: boolean;
  /** Downloads of at least this many MiB ask first; 0 asks before every download. */
  meteredWarnMib: number;
  /** Monthly traffic limit in MiB; 0 is no limit. */
  monthlyLimitMib: number;
  /** Anonymous crash statistics go to the developers; off by default (D-061). */
  shareStats: boolean;
  /** Random ID sent with crash reports only; null until the first report. */
  installId: string | null;
  /** Where downloads come from (D-062). */
  downloadSource: DownloadSource;
  /** Launcher updates: install on close, only tell, or never check (D-063). */
  updateMode: UpdateMode;
  /** Stable versions, or beta ones first (D-063). */
  updateChannel: UpdateChannel;
  writable: boolean;
}

export type UpdateMode = 'auto' | 'notify' | 'off';
export type UpdateChannel = 'stable' | 'beta';

/** Where a launcher update stands (D-063). */
export type UpdatePhase = 'idle' | 'available' | 'downloading' | 'ready' | 'notice' | 'failed';

export interface UpdateView {
  /** This build has the update key and looks for updates. */
  configured: boolean;
  phase: UpdatePhase;
  version: string | null;
  /** Release notes by language tag; `en` is the fallback. */
  notes: Record<string, string>;
  mandatory: boolean;
  /** Where to get the version by hand. */
  downloadUrl: string | null;
  downloaded: number;
  total: number | null;
  /** Why the last step failed. */
  error: 'network' | 'signature' | 'install' | null;
  /** Notes of the version now running, shown once after an update. */
  whatsNew: { version: string; notes: Record<string, string> } | null;
  /** The launcher went back to `version`: `failed` did not start. */
  rolledBack: { failed: string; version: string } | null;
}

/** Origins with the mirror as fallback, the mirror first, or never the mirror (D-062). */
export type DownloadSource = 'automatic' | 'mirror_first' | 'origin_only';

/** Who plays: a Microsoft account (Minecraft profile ID), a no-token profile (D-045)
 * or the launcher account (D-064). */
export type ProfileRef =
  | { kind: 'microsoft'; id: string }
  | { kind: 'notoken'; id: string }
  | { kind: 'launcher'; id: string };

/** What went wrong with the launcher account. */
export type LauncherAccountProblem =
  | 'network'
  | 'sign_in_again'
  | 'nick_taken'
  | 'nick_reserved'
  | 'nick_invalid'
  | 'too_many'
  | 'vault'
  | 'server';

/** The launcher account at oauth.lapi.sh (D-064). Never contains tokens. */
export interface LauncherAccountView {
  /** Signed in on this computer. */
  id: string | null;
  nick: string | null;
  emailVerified: boolean;
  /** The sign-in window is open. */
  signingIn: boolean;
  /** The saved sign-in no longer works. */
  needsSignIn: boolean;
  problem: LauncherAccountProblem | null;
}

/** Why a friends action failed (D-064). */
export type FriendsProblem =
  | 'signed_out'
  | 'network'
  | 'no_such_nick'
  | 'yourself'
  | 'you_blocked'
  | 'limit'
  | 'too_many'
  | 'not_friends'
  | 'not_owner'
  | 'invalid'
  | 'server';

export const FRIENDS_PROBLEMS: readonly FriendsProblem[] = [
  'signed_out',
  'network',
  'no_such_nick',
  'yourself',
  'you_blocked',
  'limit',
  'too_many',
  'not_friends',
  'not_owner',
  'invalid',
  'server',
];

/** Someone in a friends list; `at` is when (Unix seconds) they became one. */
export interface FriendPerson {
  id: string;
  nick: string;
  at: number;
}

export interface RoomMember {
  id: string;
  nick: string;
}

/** A chat message as the friends server stored it; `body` is plain text. */
export interface ChatMessage {
  id: number;
  room: string;
  sender: string;
  body: string;
  /** Unix seconds. */
  created: number;
}

export interface ChatRoom {
  id: string;
  /** Two friends, or a group of three or more. */
  kind: 'direct' | 'group';
  name: string;
  owner: string;
  members: RoomMember[];
  last: ChatMessage | null;
  unread: number;
}

/** Friends and chats of the launcher account (friends.lapi.sh, D-064). */
export interface FriendsView {
  signedIn: boolean;
  /** The live connection is open. */
  connected: boolean;
  me: RoomMember | null;
  friends: FriendPerson[];
  incoming: FriendPerson[];
  outgoing: FriendPerson[];
  blocked: FriendPerson[];
  rooms: ChatRoom[];
  unread: number;
  problem: FriendsProblem | null;
}

export type FriendAction = 'accept' | 'decline' | 'cancel' | 'remove' | 'block' | 'unblock';

/** A request was sent, or it met theirs and you are friends now. */
export type FriendRequestResult = 'sent' | 'friends';

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
  launcherAccount: LauncherAccountView;
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

/** What an installation runs, for its recommended memory (D-058). */
export type Workload = { kind: 'legacy' } | { kind: 'vanilla' } | { kind: 'modded'; mods: number };

export interface MemoryRecommendation {
  mib: number;
  workload: Workload;
  /** What the workload asks for, when this computer cannot give that much. */
  wantedMib: number | null;
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
  /** The Unix signal that ended the game, if any. */
  signal: number | null;
  /** Why the game stopped, when it crashed (D-059). */
  diagnosis: CrashDiagnosis | null;
}

/** Known causes of a crash, most specific first (D-059). */
export type CrashKind =
  | 'jvm_option'
  | 'heap_unavailable'
  | 'java_too_old'
  | 'java_too_new'
  | 'missing_dependency'
  | 'wrong_dependency_version'
  | 'wrong_minecraft_version'
  | 'loader_too_old'
  | 'incompatible_mods'
  | 'duplicate_mods'
  | 'mod_for_other_loader'
  | 'corrupt_mod_file'
  | 'corrupt_config'
  | 'mixin_failed'
  | 'mod_failed_to_load'
  | 'optifine_conflict'
  | 'out_of_memory'
  | 'pack_needs_memory'
  | 'system_memory'
  | 'killed'
  | 'opengl_unsupported'
  | 'graphics_driver'
  | 'overlay'
  | 'native_crash'
  | 'missing_native_library'
  | 'broken_installation'
  | 'disk_full'
  | 'file_access'
  | 'world_load_failed'
  | 'ticking_entity'
  | 'ticking_block_entity'
  | 'server_hang'
  | 'debug_crash'
  | 'game_crash'
  | 'terminated'
  | 'console_question'
  | 'unknown';

/** What the player can do about a crash from the launcher. */
export type CrashAction =
  | 'open_mods'
  | 'open_config'
  | 'open_crash_reports'
  | 'open_game_folder'
  | 'memory_settings'
  | 'java_settings'
  | 'loader_settings'
  | 'repair'
  | 'backups';

export interface CrashNote {
  kind: CrashKind;
  details: Record<string, string>;
}

export interface CrashDiagnosis {
  kind: CrashKind;
  /** Values for the explanation: mods, versions, files, Java versions. */
  details: Record<string, string>;
  /** The lines that show the cause, as the game printed them. */
  evidence: string[];
  /** Mods the crash report or stack trace points at. */
  suspects: string[];
  /** The crash report or JVM error file of this run, by file name. */
  report: string | null;
  actions: CrashAction[];
  notes: CrashNote[];
  /** Groups the same crash across players (D-061); empty for older runs. */
  signature?: string;
}

/** A folder inside an installation's game folder that the launcher opens. */
export type GameFolder =
  'mods' | 'config' | 'crash-reports' | 'logs' | 'saves' | 'resourcepacks' | 'shaderpacks';

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
  /** Whether the internet can be reached (D-052). */
  network: NetworkStatus;
  /** Download hosts served by the mirror now (D-062). */
  mirroredHosts: string[];
  /** A launcher update and where it stands (D-063). */
  update: UpdateView;
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
  | 'cancel_requested'
  | 'awaiting_approval';

/** A download that waits for the user's answer on a metered connection (D-051). */
export interface ApprovalRequest {
  bytes: number;
  files: number;
  reason: 'large' | 'over_limit';
  /** Downloaded this month before this download. */
  monthBytes: number;
  /** 0 is no limit. */
  monthlyLimitMib: number;
}

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
  /** Set while the operation waits for the user's answer (D-051). */
  approval: ApprovalRequest | null;
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

export interface RenameInstancePayload {
  instanceId: string;
  displayName: string;
}

export interface UpdateInstanceSettingsPayload {
  instanceId: string;
  maxHeapMib: number;
  resolution: Resolution;
  /** Left unchanged when omitted. */
  fullscreen?: boolean;
  gameArguments?: string;
}

export interface SettingsPatch {
  theme?: Theme;
  duringGameMode?: DuringGameMode;
  selectedInstanceId?: string;
  defaultMaxHeapMib?: number;
  defaultResolution?: Resolution;
  defaultFullscreen?: boolean;
  javaProvider?: JavaProvider;
  language?: Language;
  backupAfterPlay?: boolean;
  backupKeep?: number;
  metered?: boolean;
  meteredWarnMib?: number;
  monthlyLimitMib?: number;
  shareStats?: boolean;
  /** Replace the install ID with a new random one. */
  newInstallId?: boolean;
  downloadSource?: DownloadSource;
  updateMode?: UpdateMode;
  updateChannel?: UpdateChannel;
}

interface EventEnvelope {
  sessionEpoch: string;
  revision: number;
  timestamp: string;
}

export interface InstanceChangedEvent extends EventEnvelope {
  instance: InstanceSummary;
}

/** An installation was deleted (D-055). */
export interface InstanceRemovedEvent extends EventEnvelope {
  instanceId: string;
}

export interface SettingsChangedEvent extends EventEnvelope {
  settings: SettingsView;
}

export interface GameStateChangedEvent extends EventEnvelope {
  currentRun: RunSummary | null;
  lastRun: RunSummary | null;
  activeOperation: ActiveActivity | null;
  queue: QueuedOperation[];
  network: NetworkStatus;
  mirroredHosts: string[];
  update: UpdateView;
}

/** Whether the launcher can reach the internet; `unknown` until the first check. */
export type NetworkStatus = 'unknown' | 'online' | 'offline';

export interface AccountStateChangedEvent extends EventEnvelope {
  account: AccountSummary;
}

export interface OperationFinishedEvent extends EventEnvelope {
  result: OperationResult;
  activeOperation: ActiveActivity | null;
}

export type BackendEvent =
  | { name: 'instance_changed'; payload: InstanceChangedEvent }
  | { name: 'instance_removed'; payload: InstanceRemovedEvent }
  | { name: 'settings_changed'; payload: SettingsChangedEvent }
  | { name: 'game_state_changed'; payload: GameStateChangedEvent }
  | { name: 'operation_progress'; payload: OperationProgress }
  | { name: 'operation_finished'; payload: OperationFinishedEvent }
  | { name: 'account_state_changed'; payload: AccountStateChangedEvent };

export type BackendEventName = BackendEvent['name'];

export const BACKEND_EVENT_NAMES: readonly BackendEventName[] = [
  'instance_changed',
  'instance_removed',
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

export type BackupReason = 'manual' | 'after_play' | 'before_restore' | 'before_delete';

export interface BackupInfo {
  id: string;
  instanceId: string;
  instanceName: string;
  versionLabel: string;
  /** Game version and loader, to bring back a deleted installation (D-055). */
  versionId?: string;
  loader?: LoaderChoice;
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

/** What deleting an installation would lose (D-055). */
export interface UserData {
  worlds: string[];
  mods: number;
  resourcePacks: number;
  shaderPacks: number;
  screenshots: number;
  /** Every file of the game folder. */
  bytes: number;
}

/** One installation and what the user made in it (D-057). */
export interface InstallationData {
  instanceId: string;
  displayName: string;
  versionLabel: string;
  data: UserData;
}

/** What each part of a reset would remove (D-057). */
export interface ResetPreview {
  installations: InstallationData[];
  gameFilesBytes: number;
  backupCount: number;
  backupBytes: number;
  microsoftAccounts: number;
  notokenProfiles: number;
  temporaryBytes: number;
}

/** The parts of the launcher to reset. */
export interface ResetChoice {
  settings: boolean;
  installations: boolean;
  /** Back up installations holding worlds, mods, packs or screenshots first. */
  backUpInstallations: boolean;
  gameFiles: boolean;
  accounts: boolean;
  backups: boolean;
  traffic: boolean;
  temporary: boolean;
}

export interface ResetReport {
  backedUp: number;
  deletedInstallations: number;
  deletedBackups: number;
  freedBytes: number;
  signedOut: number;
}

/** One screenshot file of an installation (D-050). */
export interface Screenshot {
  file: string;
  takenAt: string;
  bytes: number;
  width: number | null;
  height: number | null;
}

export interface Gallery {
  /** Installations with screenshots, the one with the newest first; each newest first. */
  installations: { instanceId: string; screenshots: Screenshot[] }[];
  totalBytes: number;
}

/** Bytes the launcher downloaded, by source (D-051). */
export interface TrafficUsage {
  /** Game files and Mojang's Java. */
  game: number;
  /** Java from Adoptium. */
  java: number;
  loaders: number;
  /** Version lists and other metadata. */
  metadata: number;
}

export interface TrafficSummary {
  /** `YYYY-MM`. */
  month: string;
  thisMonth: TrafficUsage;
  /** Newest first. */
  earlier: { month: string; bytes: number }[];
}

// ---------------------------------------------------------------- crash reports (D-061)

/** What a report is about: a crashed game run or a failure of the launcher. */
export type ReportSubject =
  { type: 'game'; runId: string; instanceId: string } | { type: 'launcher'; failureId: string };

export interface ReportFile {
  name: string;
  /** Already scrubbed: what would be sent. */
  text: string;
}

/** How much the scrubbing replaced. */
export interface ScrubCounts {
  chatLines: number;
  names: number;
  addresses: number;
  worlds: number;
  ids: number;
}

export interface ReportPreview {
  id: string;
  source: 'game' | 'launcher';
  kind: string;
  signature: string;
  game: {
    version: string;
    loader: string;
    loaderVersion: string;
    java: string;
    mods: number;
  } | null;
  files: ReportFile[];
  removed: ScrubCounts;
}

/** `queued`: the service could not be reached; the report goes later by itself. */
export type Delivery = 'sent' | 'queued';

export type FailureKind = 'panic' | 'frontend' | 'unclean_exit';

/** A failure of the launcher itself, newest first. */
export interface FailureSummary {
  id: string;
  kind: FailureKind;
  at: string;
  version: string;
  message: string;
  reported: boolean;
}
