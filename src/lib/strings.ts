// User-facing English strings. Kept in one module so a later localization pass can
// replace this object without touching components.
import type {
  AccountState,
  BuildProfile,
  LifecycleScenario,
  RunState,
  DuringGameMode,
  InstallationHealth,
  MicrosoftClientIdStatus,
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
    settings: 'Settings',
    about: 'About',
  },

  bootstrap: {
    loading: 'Loading launcher state…',
    errorTitle: 'The launcher core could not be reached',
    retry: 'Try again',
    unavailableTitle: 'Launcher data is unavailable',
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
    notImplemented:
      'Installing and playing are not available in this build yet. Installation records and their settings are saved now.',
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
