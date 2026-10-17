// User-facing English strings. Kept in one module so a later localization pass can
// replace this object without touching components.
import type {
  AccountState,
  BuildProfile,
  LifecycleScenario,
  RunState,
  DuringGameMode,
  InstallationHealth,
  InstallState,
  MicrosoftClientIdStatus,
  OperationKind,
  Theme,
} from './contracts';

export const strings = {
  productName: 'Kurasis Launcher',
  buildTag: 'Development build',
  independentNotice:
    'Kurasis Launcher is an independent project. It is not affiliated with or endorsed by Mojang Studios or Microsoft.',

  nav: {
    label: 'Main',
    home: 'Installations',
    account: 'Account',
    settings: 'Settings',
    about: 'About',
  },

  bootstrap: {
    loading: 'Loading launcher state…',
    errorTitle: 'The launcher core could not be reached',
    retry: 'Try again',
    unavailableTitle: 'Launcher data is unavailable',
  },

  accountView: {
    title: 'Microsoft account',
    signIn: 'Sign in with Microsoft',
    signInAgain: 'Sign in again',
    signInHint:
      'Sign-in opens your web browser. The launcher never sees your password; it only receives a sign-in code from Microsoft.',
    notConfigured:
      'Sign-in is unavailable in this build because no project-owned Microsoft application ID is configured (see docs/AUTH-SETUP.md).',
    waiting: 'Finish signing in in your web browser. This window updates automatically.',
    cancel: 'Cancel sign-in',
    checking: 'Checking your account…',
    verified: 'Minecraft: Java Edition access verified for this session.',
    verifiedNote: 'Access is checked again every time you start the game.',
    signOut: 'Sign out',
    signOutHint:
      'Signing out removes the saved sign-in from this computer. It does not sign you out of Microsoft in your browser.',
    retrySignOut: 'Try signing out again',
    checkAgain: 'Check again',
    sessionOnly:
      'The system credential store is unavailable, so you will need to sign in again after the launcher restarts.',
    links: {
      buy_minecraft: 'Get Minecraft',
      create_profile: 'Create your Java profile',
      xbox_profile: 'Open Xbox account',
      family_settings: 'Microsoft family settings',
      minecraft_help: 'Minecraft help',
      minecraft_eula: 'Minecraft EULA',
    },
    stateTitle: {
      signed_out: 'Not signed in',
      signing_in: 'Signing in',
      refreshing: 'Checking account',
      verified_access: 'Signed in',
      no_access: 'No Java Edition access',
      profile_setup_required: 'Java profile needed',
      reauth_required: 'Sign-in required',
      service_unavailable: 'Account service unavailable',
      app_not_approved: 'Launcher not approved by Minecraft services',
      restricted: 'Account restricted',
      protocol_error: 'Account check failed',
      sign_out_pending: 'Sign-out incomplete',
    } satisfies Record<AccountState, string>,
  },

  account: {
    label: 'Account',
    state: {
      signed_out: 'Not signed in',
      signing_in: 'Signing in…',
      refreshing: 'Checking account…',
      verified_access: 'Signed in',
      no_access: 'No Java Edition access',
      profile_setup_required: 'Profile setup required',
      reauth_required: 'Sign-in required',
      service_unavailable: 'Account service unavailable',
      app_not_approved: 'Launcher not approved',
      restricted: 'Account restricted',
      protocol_error: 'Account check failed',
      sign_out_pending: 'Sign-out incomplete',
    } satisfies Record<AccountState, string>,
  },

  instances: {
    title: 'Installations',
    empty: 'No installations yet. Create one to choose a Minecraft version.',
    selected: 'Selected',
    create: 'New installation',
    health: {
      not_installed: 'Not installed',
      ready: 'Ready',
      needs_verification: 'Needs verification',
      repair_required: 'Repair required',
      unsupported: 'Unsupported',
    } satisfies Record<InstallationHealth, string>,
    busy: 'Busy',
    notImplemented: 'Playing is not available in this build yet.',
  },

  install: {
    title: 'Game files',
    install: 'Install',
    retry: 'Retry install',
    repair: 'Repair',
    verify: 'Verify files',
    cancel: 'Cancel',
    cancelling: 'Cancelling…',
    otherBusy: 'Another operation is running. Wait for it to finish or cancel it.',
    explain: {
      not_installed:
        'The game files, the Java runtime and the version metadata are downloaded from Mojang and Adoptium and checked before use.',
      ready: 'All files were verified. Repair checks every file again and replaces damaged ones.',
      needs_verification:
        'The files have not been fully verified since the last change. Verify them before playing.',
      repair_required: 'Some files are damaged or missing. Repair replaces them.',
      unsupported: 'This installation was created by a newer launcher version.',
    } satisfies Record<InstallationHealth, string>,
    userData: 'Repair never changes worlds, screenshots or options in the game folder.',
    phase: {
      resolving: 'Reading version metadata',
      checking_files: 'Checking files',
      downloading: 'Downloading',
      extracting_runtime: 'Preparing Java',
      finalizing: 'Finishing',
      cancel_requested: 'Cancelling',
    } satisfies Record<InstallState, string>,
    progressLabel: 'Download progress',
    bytes: (done: string, total: string) => `${done} of ${total}`,
    files: (done: number, total: number) => `${String(done)} of ${String(total)} files verified`,
    speed: (rate: string) => `${rate}/s`,
    result: {
      succeeded: (kind: OperationKind, files: number, bytes: string, reused: number) =>
        `${kind === 'repair' ? 'Repair' : 'Installation'} finished. Downloaded ${String(files)} files (${bytes}); reused ${String(reused)} verified files.`,
      cancelled: 'Cancelled. Files verified so far were kept; continue to resume.',
      interrupted: 'The launcher stopped before this finished. Files verified so far were kept.',
    },
  },

  createForm: {
    title: 'New installation',
    name: 'Name',
    version: 'Minecraft version',
    pending: 'pending verification',
    isolation:
      'Each installation gets its own game folder for worlds, options and screenshots. The version cannot be changed later; create another installation to play a different version.',
    submit: 'Create installation',
    cancel: 'Cancel',
    availability: {
      available: 'Available from Mojang',
      descriptor_changed: 'Changed upstream; waiting for verification',
      not_in_catalog: 'Not in the Mojang catalog',
      catalog_unavailable: 'Mojang catalog not reachable',
      checking: 'Checking the Mojang catalog…',
    },
    stale: 'Showing the last saved catalog because it could not be refreshed.',
    refresh: 'Check again',
    estimate: 'Estimate download',
    estimating: 'Estimating…',
    planTitle: (version: string) => `Download estimate for ${version}`,
    planJava: 'Java',
    planLibraries: 'Libraries',
    planAssets: 'Asset files',
    planGameFiles: 'Game files',
    planMissing: 'Still to download',
    planRuntime: 'Java runtime size',
    planRuntimeUnknown: 'Determined when the runtime is prepared',
    planNote:
      'Based on the official metadata only. The version has not yet been checked by starting the game.',
  },

  details: {
    title: 'Installation settings',
    version: 'Version',
    status: 'Status',
    memory: 'Maximum memory (MiB)',
    memoryHint: (min: number, max: number, recommended: number) =>
      `Between ${String(min)} and ${String(max)} MiB on this computer. Recommended: ${String(recommended)} MiB.`,
    lowMemory:
      'This computer has less than 8 GiB of memory. The game may run poorly with the available amount.',
    width: 'Window width',
    height: 'Window height',
    resolutionHint: (minW: number, maxW: number, minH: number, maxH: number) =>
      `Width ${String(minW)}–${String(maxW)}, height ${String(minH)}–${String(maxH)} pixels.`,
    save: 'Save changes',
    saved: 'Saved.',
    busy: 'This installation is busy; its settings cannot change right now.',
  },

  settings: {
    title: 'Settings',
    theme: 'Theme',
    themes: {
      system: 'Match system',
      light: 'Light',
      dark: 'Dark',
    } satisfies Record<Theme, string>,
    duringGame: 'While a game is running',
    duringGameModes: {
      unload_and_return: 'Close the launcher window and reopen it when the game exits',
      keep_open: 'Keep the launcher window open',
    } satisfies Record<DuringGameMode, string>,
    dataLocation: 'Launcher data location',
    readOnly:
      'Settings are read-only because they were written by a newer launcher version or could not be read.',
  },

  storage: {
    issuesTitle: 'Storage notices',
  },

  runs: {
    currentTitle: 'Running',
    lastTitle: 'Last run',
    kind: {
      lifecycle_test: 'Lifecycle test child',
      game: 'Minecraft',
    },
    state: {
      preflight: 'Preparing',
      spawning: 'Starting',
      running: 'Running',
      stopping: 'Stopping',
      exited: 'Exited normally',
      failed: 'Exited with an error',
      cancelled: 'Cancelled',
    } satisfies Record<RunState, string>,
    exitCode: 'Exit code',
    duration: 'Duration',
    seconds: (ms: number) => `${(ms / 1000).toFixed(1)} s`,
    drainTimedOut:
      'Output capture was closed 2 seconds after exit because another process kept the output open.',
    outputTitle: 'Last output lines',
    dropped: (n: number) => `${String(n)} earlier lines not shown.`,
    stop: 'Stop',
  },

  harness: {
    title: 'Lifecycle test (debug build only)',
    explanation:
      'Starts a harmless copy of this launcher as a child process to test closing and reopening the window. It cannot run any other program and is not part of release builds.',
    scenarios: {
      immediate_exit: 'Exit immediately',
      short: 'Run 4 seconds',
      long: 'Run 12 seconds',
      crash: 'Crash after 4 seconds',
      inherited_pipe: 'Keep output pipes open',
      noisy: 'Heavy output',
    } satisfies Record<LifecycleScenario, string>,
  },

  quit: {
    button: 'Quit',
    dialogTitle: 'A run is still active',
    dialogBody:
      'Quitting now would stop it. Stopping a game may lose unsaved progress. You can keep the launcher running in the background instead; open it again to return.',
    keep: 'Keep running in background',
    stop: 'Stop and quit',
    cancel: 'Cancel',
  },

  foundation: {
    title: 'Project status',
    body: 'Sign-in, game installation and launching are not implemented yet, so no play controls are shown.',
  },

  systemInfo: {
    title: 'Launcher core',
    loading: 'Contacting the launcher core…',
    errorPrefix: 'The launcher core could not be reached:',
    retry: 'Try again',
    version: 'Version',
    platform: 'Platform',
    build: 'Build',
    framework: 'Tauri',
    buildProfile: {
      debug: 'Debug',
      release: 'Release',
    } satisfies Record<BuildProfile, string>,
  },

  microsoftSetup: {
    title: 'Microsoft sign-in setup',
    status: {
      not_configured: 'Not configured',
      invalid: 'Invalid client ID',
      configured_unverified: 'Configured, not yet verified',
    } satisfies Record<MicrosoftClientIdStatus, string>,
    explanation: {
      not_configured:
        'This build has no project-owned Microsoft application ID. Sign-in will stay unavailable until the project owner registers one. See docs/AUTH-SETUP.md.',
      invalid:
        'The Microsoft application ID supplied at build time is not a valid GUID. Rebuild with a correct value. See docs/AUTH-SETUP.md.',
      configured_unverified:
        'A Microsoft application ID is present. It has not been verified against Microsoft and Minecraft services yet.',
    } satisfies Record<MicrosoftClientIdStatus, string>,
  },
} as const;
