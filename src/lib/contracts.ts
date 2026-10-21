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
  /** Launcher Skins is added to the game where a jar exists for it (D-065). */
  launcherSkins: boolean;
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
export type LoaderKind = 'fabric' | 'quilt' | 'forge' | 'neoforge' | 'optifine';

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
  /** A short sound for a new chat message or friend request (D-064). */
  chatSound: boolean;
  /** A system notification for one while the launcher is not in front. */
  chatNotifications: boolean;
  /** The launcher account plays offline in the full game instead of the demo
   * (only while a Microsoft account that owns Minecraft is signed in). */
  launcherAccountOffline: boolean;
  /** The same for the profiles without an account. */
  notokenOffline: boolean;
  /** Random ID sent with crash reports only; null until the first report. */
  installId: string | null;
  /** Where downloads come from (D-062). */
  downloadSource: DownloadSource;
  /** Launcher updates: install on close, only tell, or never check (D-063). */
  updateMode: UpdateMode;
  /** Stable versions, or beta ones first (D-063). */
  updateChannel: UpdateChannel;
  /** The screen opened at start. */
  startView: StartView;
  /** The last start screen opened, for `startView: 'last'`. */
  lastView: RememberedView;
  /** The start page feed (D-067). */
  feed: FeedSettings;
  /** Worlds connect through the relay only, never directly (W10). */
  worldsRelayOnly: boolean;
  writable: boolean;
}

/** Screens the launcher can open at start; `last` is the one open last time.
 * The core checks the value against this closed set. */
export type StartView = 'feed' | 'home' | 'mods' | 'skins' | 'account' | 'last';
/** A start screen that can be remembered as the last one opened. */
export type RememberedView = Exclude<StartView, 'last'>;

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
  | 'not_saved'
  | 'server';

export const LAUNCHER_ACCOUNT_PROBLEMS: readonly LauncherAccountProblem[] = [
  'network',
  'sign_in_again',
  'nick_taken',
  'nick_reserved',
  'nick_invalid',
  'too_many',
  'vault',
  'not_saved',
  'server',
];

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
  | 'muted'
  | 'banned'
  | 'email_not_confirmed'
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
  'muted',
  'banned',
  'email_not_confirmed',
  'invalid',
  'server',
];

/** Someone in a friends list; `at` is when (Unix seconds) they became one. */
export interface FriendPerson {
  id: string;
  nick: string;
  at: number;
  /** A friend with the launcher open (unless they hide it). */
  online: boolean;
  /** What a friend plays now, as their launcher names it. */
  playing: string | null;
}

/** A mute or ban a moderator put on the account; `until` 0 is for good. */
export interface FriendsSanction {
  kind: 'mute' | 'ban';
  until: number;
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
  /** Friends see that this account is online. */
  visible: boolean;
  sanction: FriendsSanction | null;
  problem: FriendsProblem | null;
}

export type FriendAction = 'accept' | 'decline' | 'cancel' | 'remove' | 'block' | 'unblock';

/** The launcher account's skin or cape (skins.lapi.sh, D-065). */
export type SkinKind = 'skin' | 'cape';

/** Why a skins action failed; the service's refusals keep its codes. */
export type SkinsProblem =
  | 'signed_out'
  | 'network'
  | 'unreadable'
  | 'bad_png'
  | 'bad_size'
  | 'bad_frames'
  | 'bad_frame_ms'
  | 'bad_model'
  | 'too_large'
  | 'rate_limited'
  | 'invalid'
  | 'server';

export const SKINS_PROBLEMS: readonly SkinsProblem[] = [
  'signed_out',
  'network',
  'unreadable',
  'bad_png',
  'bad_size',
  'bad_frames',
  'bad_frame_ms',
  'bad_model',
  'too_large',
  'rate_limited',
  'invalid',
  'server',
];

/** A worn texture: a vertical strip of `frames` frames. */
export interface WornTexture {
  /** SHA-256 of the PNG; `skinsTexture` fetches it. */
  hash: string;
  /** Skins only: the slim (3-pixel) arms. */
  slim: boolean;
  width: number;
  height: number;
  frames: number;
  /** One per frame; empty for a single frame. */
  frameMs: number[];
}

export interface Wardrobe {
  skin: WornTexture | null;
  cape: WornTexture | null;
}

/** A PNG the player picked and has not uploaded yet, checked by the core. */
export interface PickedTexture {
  kind: SkinKind;
  /** The file's name, plain text. */
  name: string;
  width: number;
  height: number;
  frames: number;
  /** The file as a `data:` URL. */
  preview: string;
}

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
  /** A Microsoft account that owns Minecraft is signed in: players without a
   * token may play offline in the full game. */
  fullGameAllowed: boolean;
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
  /** The server the game last joined (`host:port`): friends can be invited to it. */
  server: string | null;
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
  | { status: 'choice_required'; operationId: string; hosting: boolean };

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
  /** Launcher Skins in the game (D-065); left unchanged when omitted. */
  launcherSkins?: boolean;
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
  chatSound?: boolean;
  chatNotifications?: boolean;
  launcherAccountOffline?: boolean;
  notokenOffline?: boolean;
  /** Replace the install ID with a new random one. */
  newInstallId?: boolean;
  downloadSource?: DownloadSource;
  updateMode?: UpdateMode;
  updateChannel?: UpdateChannel;
  startView?: StartView;
  lastView?: RememberedView;
  /** Fields omitted stay as they are. */
  feed?: Partial<FeedSettings>;
  worldsRelayOnly?: boolean;
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

export type BackupReason =
  'manual' | 'after_play' | 'before_restore' | 'before_delete' | 'before_update';

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
  /** Files from the catalog of mods (D-066). */
  mods: number;
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

/** A file of a review (D-070): scrubbed text, or a PNG or JPEG in base64. */
export interface ReviewFile {
  name: string;
  kind: 'text' | 'image';
  text?: string;
  data?: string;
}

/** Texts found or read for a review, scrubbed, and what the scrubbing replaced. */
export interface ReviewFound {
  files: ReviewFile[];
  removed: ScrubCounts;
}

export type ReviewKind = 'bug' | 'idea' | 'question' | 'impression';

/** What the review form sends. */
export interface ReviewDraft {
  kind: ReviewKind;
  category: string;
  subcategory: string | null;
  message: string;
  rating: number | null;
  linkAccount: boolean;
  diagnosticId: string | null;
  ad: number | null;
  files: ReviewFile[];
}

export type ReviewStatus = 'sent' | 'read' | 'answered' | 'closed' | 'gone';

/** A review this computer sent, with what the service last said of it. */
export interface SentReview {
  id: number;
  sentAt: string;
  kind: ReviewKind;
  category: string;
  subcategory: string;
  excerpt: string;
  account: boolean;
  status: ReviewStatus;
  reply: string | null;
  replied: string | null;
  replySeen: boolean;
}

export interface MyReviews {
  reviews: SentReview[];
  /** Answers not read yet. */
  unseen: number;
  /** Reviews waiting to be sent. */
  waiting: number;
}

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

// ----- the catalog of mods (D-066) -----

/** What the catalog holds. */
export type ModsKind = 'mod' | 'modpack' | 'resourcepack' | 'shader' | 'datapack' | 'world';
export type ModsSource = 'modrinth' | 'curseforge' | 'packwiz' | 'own' | 'external';
export type ModsSort = 'relevance' | 'downloads' | 'updated' | 'new' | 'title';

export interface ModsSearchQuery {
  text: string;
  kind: ModsKind | null;
  game: string | null;
  loader: string | null;
  category: string | null;
  sort: ModsSort | null;
  offset: number;
  limit: number | null;
}

/** A project in a list. */
export interface ModsHit {
  id: string;
  kind: ModsKind;
  slug: string;
  title: string;
  summary: string;
  /** Only from files-mods.lapi.sh. */
  icon: string | null;
  downloads: number;
  /** `importing`: still being brought in. */
  status: 'importing' | 'ready';
  /** Unix seconds. */
  updated: number;
  author: string | null;
  sources: ModsSource[];
}

export interface ModsSearchPage {
  total: number;
  hits: ModsHit[];
}

/**
 * A description as safe nodes (never HTML): `k` is the element; `a` has `href`
 * (https only), `img` has `href` (files-mods.lapi.sh only) and `text` (its
 * description).
 */
export interface DocNode {
  k: string;
  text?: string;
  href?: string;
  c?: DocNode[];
}

export interface ModsProject {
  id: string;
  kind: ModsKind;
  slug: string;
  title: string;
  summary: string;
  body: DocNode[];
  license: string;
  icon: string | null;
  clientSide: 'required' | 'optional' | 'unsupported' | 'unknown';
  serverSide: 'required' | 'optional' | 'unsupported' | 'unknown';
  status: 'importing' | 'ready';
  downloads: number;
  followers: number;
  published: number;
  updated: number;
  /** When the source stopped having it; the catalog keeps it. */
  goneAt: number | null;
  authors: { name: string; role: string }[];
  categories: string[];
  sources: { source: ModsSource; url: string | null }[];
  gallery: { url: string; thumb: string | null; title: string; description: string }[];
}

export interface ModsVersionFile {
  name: string;
  size: number;
  sha256: string | null;
  primary: boolean;
  url: string | null;
  /** Held back by the catalog's malware check: not installed. */
  held: boolean;
}

export interface ModsVersion {
  id: string;
  projectId: string;
  number: string;
  name: string;
  /** Empty in lists. */
  changelog: DocNode[];
  channel: 'release' | 'beta' | 'alpha';
  published: number;
  downloads: number;
  /** `all`: kept; otherwise the catalog fetches the files on first install. */
  saved: 'all' | 'partial' | 'none';
  games: string[];
  loaders: string[];
  files: ModsVersionFile[];
  dependencies: {
    kind: 'required' | 'optional' | 'incompatible' | 'embedded';
    projectId: string | null;
    versionId: string | null;
  }[];
  goneAt: number | null;
}

export interface ModsCandidate {
  source: ModsSource;
  id: string;
  kind: ModsKind | null;
  title: string;
  summary: string;
  author: string;
  downloads: number;
  /** Already in the catalog. */
  projectId: string | null;
  /** Its icon and poster, kept by the catalog. */
  icon: string | null;
  banner: string | null;
}

/** A request to add to the catalog. */
export interface ModsImportRequest {
  id: string;
  input: string;
  state: 'queued' | 'choosing' | 'importing' | 'ready' | 'failed';
  error: string | null;
  projectId: string | null;
  candidates: ModsCandidate[];
  created: number;
  finished: number | null;
}

export type ModsMisfit = 'no_loader' | 'wrong_loader' | 'wrong_game';

export interface ModsPlanItem {
  projectId: string;
  title: string;
  kind: ModsKind;
  icon: string | null;
  versionId: string;
  number: string;
  published: number;
  fileName: string;
  size: number;
  /** The catalog keeps the file already; otherwise it fetches it first. */
  kept: boolean;
  dependency: boolean;
  path: string;
  replaces: string | null;
  /** The same file is there already. */
  present: boolean;
}

export interface ModsPlan {
  items: ModsPlanItem[];
  missing: { projectId: string | null; title: string | null }[];
  conflicts: {
    path: string;
    reason: 'incompatible' | 'name_taken' | 'held';
    title: string | null;
  }[];
  misfit: ModsMisfit | null;
  totalBytes: number;
  /** On a metered connection, why this download asks first (D-051). */
  approval: ApprovalRequest | null;
  /** Files there but turned off that this needs: turned on with it. */
  turnOn: { path: string; title: string }[];
}

/** Which mods need one about to be turned off or removed. */
export interface ModsNeededBy {
  neededBy: string[];
  /** What the launcher added that only it needed. */
  unused: { path: string; title: string }[];
}

export interface ModsInstallPayload {
  instanceId: string;
  version: string;
  world: string | null;
  replaces: string | null;
}

/** Adding a map from the catalog as a new world. */
export interface ModsWorldPayload {
  instanceId: string;
  version: string;
}

/** What adding a map will do. */
export interface ModsWorldPlan {
  /** The world's folder name, one no world has yet. */
  name: string;
  size: number;
  /** The catalog keeps the file already. */
  kept: boolean;
  approval: ApprovalRequest | null;
}

/** What a picked instance, game folder or mods folder holds (D-066 stage 5). */
export interface ModsImportFound {
  format: 'prism' | 'folder';
  name: string;
  game: string | null;
  loader: LoaderKind | null;
  /** The version and loader were guessed from the mods. */
  guessed: boolean;
  files: number;
  bytes: number;
  mods: number;
  leftOut: number;
}

export interface ModsImportCreatePayload {
  displayName: string;
  gameVersion: string;
  loader: LoaderKind | null;
}

/** What bringing in copied. */
export interface ModsImportReport {
  files: number;
  /** Already there from an earlier try. */
  kept: number;
  leftOut: number;
}

/** `mods://progress` while an install runs. */
export interface ModsProgress {
  instanceId: string;
  phase: 'fetching' | 'downloading' | 'unpacking' | 'uploading' | 'done' | 'failed' | 'cancelled';
  files: number;
  totalFiles: number;
  bytes: number;
  totalBytes: number;
}

/** A modpack version as the catalog has it (D-066 stage 3). */
export interface ModsPack {
  state: 'none' | 'working' | 'ready' | 'failed';
  /** Why it failed: a short code. */
  error: string | null;
  game: string | null;
  loader: { kind: LoaderKind; version: string } | null;
  /** Files the catalog keeps already, of all. */
  kept: number;
  total: number;
  /** Files left out by the catalog. */
  skipped: number;
  mods: number;
  totalBytes: number;
  /** On a metered connection, why the download asks first (D-051). */
  approval: ApprovalRequest | null;
}

/** What applying a modpack did. */
export interface ModsPackReport {
  files: number;
  /** The player's own changes, left as they are. */
  keptYours: number;
  leftOut: number;
  removed: number;
}

export interface ContentItem {
  /** As on disk now, relative to the game folder. */
  path: string;
  folder: 'mods' | 'resourcepacks' | 'shaderpacks' | 'datapacks';
  /** Its name when on. */
  name: string;
  world: string | null;
  enabled: boolean;
  size: number;
  directory: boolean;
  projectId: string | null;
  versionId: string | null;
  title: string | null;
  versionNumber: string | null;
  icon: string | null;
  dependency: boolean;
  /** The modpack the installation was made from brought it. */
  fromPack: boolean;
}

/** The modpack an installation was made from. */
export interface ContentPack {
  projectId: string;
  versionId: string;
  title: string;
  versionNumber: string;
  icon: string | null;
}

export interface ContentView {
  items: ContentItem[];
  worlds: string[];
  pack: ContentPack | null;
}

/** What goes into a modpack export besides mods, packs, shaders and configs. */
export interface ModsExportChoice {
  worlds: boolean;
  options: boolean;
}

/** What a modpack export wrote. */
export interface ModsExportResult {
  /** False when the save dialog was closed. */
  saved: boolean;
  fileName: string | null;
  report: {
    /** Files named by their address on Modrinth. */
    linked: number;
    /** Files copied into the archive. */
    bundled: number;
    bytes: number;
  } | null;
}

/** What publishing an installation as a pack would take (D-066 stage 6). */
export interface ModsPublishCheck {
  check: {
    /** Files named by their hashes in the catalog. */
    linked: number;
    /** The player's own files, copied in. */
    bundled: number;
    bytes: number;
    /** Mods the catalog does not have: to be added first. */
    missing: string[];
    /** Files the catalog never takes in a pack (code, programs). */
    refused: string[];
    /** Files over the size a pack may bring. */
    tooBig: string[];
    /** The catalog's pack this installation was published as. */
    published: string | null;
  };
  /** The account's pack this installation was published as. */
  pack: ModsOwnPack | null;
  /** The installation's name, for the title. */
  name: string;
}

/** One of the player's own packs, as the catalog lists it. */
export interface ModsOwnPack {
  id: string;
  slug: string;
  title: string;
  status: 'ready' | 'hidden' | 'removed' | 'importing';
  versions: number;
  latest: string | null;
}

/** What the player fills in to publish a pack. */
export interface ModsPublishForm {
  title: string;
  summary: string;
  body: string;
  version: string;
  changes: string;
  choice: ModsExportChoice;
}

/** What the catalog answered to a publish. */
export type ModsPublished =
  | { outcome: 'published'; projectId: string; slug: string; versionId: string }
  | {
      outcome: 'refused';
      /** not_in_catalog, held, not_allowed or too_big. */
      code: string;
      paths: string[];
    };

/** What a player may report a pack for. */
export type ModsReportReason = 'malware' | 'stolen' | 'inappropriate' | 'broken' | 'other';

/** A file of the player's that the catalog recognized. */
export interface ContentIdentified {
  path: string;
  projectId: string;
  versionId: string;
}

export interface ContentUpdate {
  path: string;
  projectId: string;
  fromVersionId: string;
  toVersionId: string;
  toNumber: string;
}

// ---- The start page feed (D-067) ----------------------------------------------

/** The language of the feed's videos and streams: the interface's, or one of these. */
export type VideoLanguage = 'ui' | 'en' | 'ru' | 'ar';
/** Where a video or stream plays. */
export type PlayerMode = 'window' | 'browser';
/** A part of the feed the player can hide. */
export type FeedBlock = 'versions' | 'news' | 'videos' | 'streams';

export interface FeedSettings {
  /** False hides the Feed tab; the launcher then asks the feed for nothing. */
  enabled: boolean;
  /** Snapshots, pre-releases and release candidates. */
  snapshots: boolean;
  videoLanguage: VideoLanguage;
  /** YouTube Shorts among the videos. */
  shorts: boolean;
  /** Patch notes and news machine-translated into the interface language. */
  machineTranslation: boolean;
  player: PlayerMode;
  hidden: FeedBlock[];
  /** A system notification when a favourite Twitch channel goes live. */
  liveNotifications: boolean;
}

/** The feed's languages. */
export type FeedLang = 'en' | 'ru' | 'ar';
export type VersionKind = 'release' | 'snapshot' | 'pre' | 'rc';

/** A patch note (`j-…`) or a news article (`n-…`), as a card. */
export interface NoteCard {
  id: string;
  title: string;
  date: string;
  summary: string;
  /** Machine-translated from English. */
  translated: boolean;
  version: string | null;
  kind: VersionKind | null;
  /** `img/<sha>.jpg` on the `feed:` protocol. */
  image: string | null;
  /** The article on www.minecraft.net (news). */
  url: string | null;
}

export interface FeedVideo {
  id: string;
  channelId: string;
  channel: string;
  title: string;
  date: string;
  short: boolean;
  image: string | null;
}

export interface FeedStream {
  login: string;
  name: string;
  title: string;
  viewers: number;
  started: string;
  image: string | null;
}

export interface FeedCycle {
  target: string;
  snapshots: number;
  pre: number;
  rc: number;
  released: boolean;
  stage: VersionKind;
  latest: string;
  date: string;
}

export interface LatestVersion {
  id: string;
  kind: VersionKind;
  date: string;
}

export interface FeedEvent {
  id: string;
  title: string;
  starts: string;
  ends: string;
  url: string | null;
}

export interface FeedAnnouncement {
  id: string;
  title: string;
  text: string;
  url: string | null;
}

export interface Feed {
  format: number;
  generated: string;
  lang: FeedLang;
  versions: NoteCard[];
  news: NoteCard[];
  videos: FeedVideo[];
  streams: FeedStream[];
  cycle: FeedCycle | null;
  latest: { release: LatestVersion | null; snapshot: LatestVersion | null };
  events: FeedEvent[];
  announcements: FeedAnnouncement[];
  tip: string | null;
  fetched: Record<string, string>;
}

/** What went wrong with the feed; the interface says it in words. */
export type FeedProblem =
  | 'disabled'
  | 'offline'
  | 'server'
  | 'not_found'
  | 'bad_input'
  | 'too_many'
  | 'full'
  | 'unavailable';

export const FEED_PROBLEMS: readonly FeedProblem[] = [
  'disabled',
  'offline',
  'server',
  'not_found',
  'bad_input',
  'too_many',
  'full',
  'unavailable',
];

/** One item of the endless list (D-067). */
export type FeedItem =
  | { type: 'note'; note: NoteCard }
  | { type: 'video'; video: FeedVideo }
  | { type: 'stream'; stream: FeedStream };

/** A page of the endless list, newest first; `next` asks for the page after. */
export interface FeedPage {
  items: FeedItem[];
  next: string | null;
}

export interface FeedView {
  feed: Feed;
  /** A copy kept on disk is shown: the service did not answer. */
  stale: boolean;
  problem: FeedProblem | null;
}

/** An ad for a Minecraft server in the feed (D-069), read through the core's rules. */
export interface ServerAd {
  id: string;
  name: string;
  /** What Play joins: `host`, `host:port` or `[IPv6]:port`. */
  address: string;
  text: string;
  /** The version Play starts by default. */
  version: string;
  /** Every version the server lets in, newest first. */
  versions: string[];
  /** A PNG data URL. */
  icon: string | null;
  /** Whether the server answered its last ping; null before the first. */
  up: boolean | null;
  online: number | null;
  max: number | null;
  /** Unix seconds; hidden from then on. */
  endsAt: number;
  weight: number;
  erid: string | null;
  /** A JPEG data URL of a 16:9 picture shown over the icon. */
  banner: string | null;
  /** The server wants a Microsoft sign-in (online mode); null when unknown. */
  licensed: boolean | null;
}

export interface ServerAdsView {
  ads: ServerAd[];
  stale: boolean;
}

/** A run of text in a note: bold, italic, code, or a link the launcher may open. */
export interface NoteRun {
  text: string;
  b?: boolean;
  i?: boolean;
  code?: boolean;
  href?: string;
}

export type NoteBlock =
  | { t: 'h'; level: number; c: NoteRun[] }
  | { t: 'p'; c: NoteRun[] }
  | { t: 'ul'; items: NoteBlock[][] }
  | { t: 'ol'; items: NoteBlock[][] }
  | { t: 'img'; src: string; alt: string };

export interface Note extends NoteCard {
  lang: FeedLang;
  blocks: NoteBlock[];
}

export interface NoteView {
  note: Note;
  stale: boolean;
}

/** A YouTube channel the player added to the feed. */
export interface OwnChannel {
  channelId: string;
  title: string;
}

/** A Twitch channel the player added to the feed. */
export interface OwnStreamer {
  login: string;
  name: string;
}

/** Where a marked channel is. */
export type MarkKind = 'youtube' | 'twitch';

/** A channel the player hid from the feed or put first: a YouTube channel id or a Twitch login. */
export interface ChannelMark {
  kind: MarkKind;
  id: string;
  name: string;
}

/** Which list a mark puts a channel on. */
export type MarkList = 'hidden' | 'favorite';

export interface FeedMarks {
  hidden: ChannelMark[];
  favorite: ChannelMark[];
}

/** The newest videos and live streams of the favourite channels, shown first. */
export interface FavoriteFeed {
  videos: FeedVideo[];
  streams: FeedStream[];
}

// ----- Worlds with friends (D-068) -----

/** Why a call to worlds.lapi.sh or a step of a room failed. */
export type WorldsProblem =
  | 'signed_out'
  | 'network'
  | 'server'
  | 'too_many'
  | 'invalid'
  | 'not_found'
  | 'forbidden'
  | 'banned'
  | 'nick_unfit'
  | 'invite_invalid'
  | 'not_member'
  | 'limit'
  | 'world_taken'
  | 'revision_conflict'
  | 'owner_cannot_leave'
  | 'not_hosting'
  | 'session_ended'
  | 'ticket_invalid'
  | 'wrong_session'
  | 'room_full'
  | 'already_connected'
  | 'device'
  | 'refused';

export const WORLDS_PROBLEMS: readonly WorldsProblem[] = [
  'signed_out',
  'network',
  'server',
  'too_many',
  'invalid',
  'not_found',
  'forbidden',
  'banned',
  'nick_unfit',
  'invite_invalid',
  'not_member',
  'limit',
  'world_taken',
  'revision_conflict',
  'owner_cannot_leave',
  'not_hosting',
  'session_ended',
  'ticket_invalid',
  'wrong_session',
  'room_full',
  'already_connected',
  'device',
  'refused',
];

export type WorldsRole = 'owner' | 'manager' | 'member';

/** A room of worlds with friends, as the launcher account sees it. */
export interface WorldsRoom {
  id: string;
  name: string;
  description: string;
  minecraft: string;
  revision: number;
  owner: string;
  ownerNick: string;
  role: WorldsRole;
  /** The world's ID on the owner's computer. */
  world: string | null;
  members: number;
  hosting: boolean;
  online: number;
  maxPlayers: number;
  created: number;
  updated: number;
  /** The mods the room plays now, while it is hosted with them. */
  pack: WorldsSessionPack | null;
}

/** A world's room pack: the catalog version guests install, and its SHA-256. */
export interface WorldsSessionPack {
  version: string;
  sha256: string;
}

export interface WorldsMember {
  account: string;
  nick: string;
  role: WorldsRole;
  state: string;
  joined: number;
}

export interface WorldsRoomPage {
  room: WorldsRoom;
  members: WorldsMember[];
}

export interface WorldsInvite {
  id: string;
  created: number;
  expires: number;
  maxUses: number;
  uses: number;
}

export interface WorldsNewInvite {
  invite: WorldsInvite;
  /** Shown once: the service keeps only its digest. */
  code: string;
}

export interface WorldsPreview {
  room: string;
  name: string;
  ownerNick: string;
  members: number;
  minecraft: string;
}

export type WorldsDifficulty = 'peaceful' | 'easy' | 'normal' | 'hard';
export type WorldsGameMode = 'survival' | 'creative' | 'adventure';
export type WorldsPreset = 'normal' | 'flat' | 'large_biomes' | 'amplified';

export interface WorldsSettings {
  /** Empty for a random seed. */
  seed: string;
  preset: WorldsPreset;
  difficulty: WorldsDifficulty;
  gameMode: WorldsGameMode;
  hardcore: boolean;
  pvp: boolean;
  maxPlayers: number;
}

export interface LocalWorld {
  id: string;
  name: string;
  minecraft: string;
  eulaAccepted: boolean;
  serverInstalled: boolean;
  settings: WorldsSettings;
  createdAt: string;
  lastRun: { endedAt: string; clean: boolean } | null;
  /** Played at least once: only then is there a save to back up or export. */
  played: boolean;
  /** The mod loader of a world with mods, and how many mods it has. */
  loader: LoaderChoice | null;
  mods: number;
  /** Its generation of mods, and the installation they came from. */
  generation: number | null;
  source: string | null;
  /** Its mods were updated and it has not started with them yet. */
  updatePending: boolean;
}

/** A mod an update of a world's mods adds, removes or changes (W12). */
export interface WorldsModChange {
  name: string;
  side: 'client' | 'server' | 'both';
  /** The file it replaces, for a changed mod. */
  from?: string;
}

/** One side of a changed setting, as far as it is shown. */
export interface WorldsShownSetting {
  /** Null for a file that is not text. */
  text: string | null;
  bytes: number;
  truncated: boolean;
}

/** A server setting the host changed that the new mods bring another version of. */
export interface WorldsSettingConflict {
  path: string;
  /** What the current mods brought; null when they had no such file. */
  base: WorldsShownSetting | null;
  mine: WorldsShownSetting;
  theirs: WorldsShownSetting;
}

/** What updating a world's mods from an installation would change. */
export interface WorldsUpdatePreview {
  fingerprint: string;
  generation: number;
  next: number;
  loader: LoaderChoice;
  loaderChanged: boolean;
  added: WorldsModChange[];
  removed: WorldsModChange[];
  updated: WorldsModChange[];
  /** Settings that change without asking. */
  settings: number;
  conflicts: WorldsSettingConflict[];
  /** Nothing would change. */
  same: boolean;
}

/** The host's answer to a changed setting. */
export type WorldsKeep = 'mine' | 'theirs';

/** An installation whose single-player worlds can be imported. */
export interface WorldsImportable {
  instanceId: string;
  instanceName: string;
  minecraft: string;
  /** Its mod loader, whose mods the world takes. */
  loader: LoaderChoice | null;
  saves: string[];
}

/** An installation a world with mods can be made from. */
export interface WorldsModSource {
  instanceId: string;
  name: string;
  minecraft: string;
  loader: LoaderChoice;
}

export interface WorldsImportPayload {
  instanceId: string;
  save: string;
  name: string;
  description: string;
}

export interface WorldsCreatePayload {
  name: string;
  description: string;
  minecraft: string;
  settings: WorldsSettings;
  /** An installation with a mod loader whose mods the world takes. */
  instanceId?: string | null;
}

export type WorldsStage = 'server' | 'starting' | 'opening' | 'joining' | 'backing_up' | 'trying';

export type WorldsEnding = 'stopped' | 'server_exited' | 'session_lost' | 'room_gone';

export type WorldsRefusal =
  | 'BAD_FRAME'
  | 'TICKET_INVALID'
  | 'NOT_MEMBER'
  | 'ALREADY_CONNECTED'
  | 'CONNECTION_CLOSING'
  | 'WRONG_SESSION'
  | 'ROOM_FULL'
  | 'HOST_BUSY'
  | 'UNAVAILABLE';

/** What this computer does for a room (`worlds://state`). */
export interface WorldsState {
  preparing: {
    room: string;
    stage: WorldsStage;
    downloadedBytes: number;
    totalBytes: number;
  } | null;
  hosting: {
    room: string;
    world: string;
    phase: 'starting' | 'ready' | 'stopping' | 'exited';
    players: string[];
    ended: WorldsEnding | null;
    /** The lease lapsed (sleep, the service out of reach); a new session is being opened. */
    reopening: boolean;
    clean: boolean | null;
  } | null;
  guest: {
    room: string;
    connected: boolean;
    refused: WorldsRefusal | null;
    /** Straight to the host or through relay.lapi.sh, once chosen. */
    path: 'direct' | 'relay' | null;
    /** The tunnel was lost and is being opened again. */
    reconnecting: boolean;
    /** The room no longer lets this computer in: leave or join again. */
    gaveUp: boolean;
  } | null;
  /** Why the last start or join failed: a problem name or an error code. */
  failed: string | null;
  /** A world is being backed up, restored, imported or exported. */
  maintenance: boolean;
}

export interface WorldsInstance {
  instanceId: string;
  ready: boolean;
  /** The room's mods wait for the player's word: nothing was installed. */
  consent: RoomConsent | null;
}

/** What a guest agrees to before a room's mods go on their computer. */
export interface RoomConsent {
  /** The room pack, sent back with the player's yes. */
  sha256: string;
  mods: number;
  /** Mods this computer does not have yet: how many, their names, their bytes. */
  newMods: number;
  names: string[];
  bytes: number;
}

/** A computer of the launcher account for worlds. */
export interface WorldsDevice {
  id: string;
  name: string;
  /** Unix seconds. */
  created: number;
  /** This computer. */
  current: boolean;
}
