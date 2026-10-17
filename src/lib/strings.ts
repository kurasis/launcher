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
  JavaProvider,
  MicrosoftClientIdStatus,
  OperationKind,
  Theme,
  VersionType,
} from './contracts';

/** Sections of the Settings screen. */
export type SettingsSection = 'general' | 'java' | 'defaults' | 'installations';

export const strings = {
  // The header mark (D-032): "Sferum" large, "Launcher" small.
  brand: { name: 'Sferum', suffix: 'Launcher' },
  // Debug-build test profile (D-030): launches run in Minecraft's demo mode.
  testProfileTag: (name: string) => `Test profile ${name} · demo mode`,
  // Only debug cores are labelled; release builds show no build tag (D-032).
  devBuildTag: 'Development build',
  independentNotice:
    'This launcher is an independent project. It is not affiliated with or endorsed by Mojang Studios or Microsoft.',

  nav: {
    label: 'Main',
    home: 'Installations',
    account: 'Account',
    settings: 'Settings',
    diagnostics: 'Diagnostics',
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
    waiting: 'Finish signing in with your web browser. This window updates automatically.',
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
      source_code: 'Source code',
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
  },

  install: {
    install: 'Install',
    retry: 'Retry install',
    repair: 'Repair',
    verify: 'Verify files',
    play: 'Play',
    playDemo: 'Play demo',
    demoHint:
      'Without a signed-in account you can play the Minecraft demo: five in-game days, no multiplayer. Sign in with an account that owns Minecraft for the full game.',
    starting: 'Starting…',
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
    versionTypes: {
      release: 'Releases',
      snapshot: 'Snapshots',
      old_beta: 'Beta',
      old_alpha: 'Alpha',
    } satisfies Record<VersionType, string>,
    typeHints: {
      release: 'Stable versions.',
      snapshot:
        'Test versions before a release. They can damage worlds; keep copies of worlds you care about.',
      old_beta:
        'Beta versions from 2010–2011. They run on Java 8 and have no demo mode, so playing needs a signed-in account.',
      old_alpha:
        'Alpha, Classic and pre-Classic versions from 2009–2010. They run on Java 8 and have no demo mode, so playing needs a signed-in account.',
    } satisfies Record<VersionType, string>,
    search: 'Find a version',
    noMatch: 'No version matches.',
    verified: 'Verified',
    latest: 'Latest',
    selected: (id: string) => `Selected: ${id}`,
    noneSelected: 'Choose a version.',
    isolation:
      'Each installation gets its own game folder for worlds, options and screenshots. The version cannot be changed later; create another installation to play a different version.',
    submit: 'Create installation',
    cancel: 'Cancel',
    availability: {
      available: 'Available from Mojang',
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
    summaryTitle: 'Details',
    version: 'Version',
    status: 'Status',
    openFolder: 'Open game folder',
    openFolderHint: 'Worlds, screenshots and options of this installation are stored there.',
    lastResult: 'Last result',
    noResult: 'None yet',
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
    java: 'Java',
    javaNone: 'Chosen when it is installed',
    window: 'Memory and window',
    windowValue: (mib: number, width: number, height: number) =>
      `${String(mib)} MiB · ${String(width)}×${String(height)}`,
    edit: 'Change settings',
  },

  settings: {
    title: 'Settings',
    sectionsLabel: 'Settings sections',
    sections: {
      general: 'General',
      java: 'Java',
      defaults: 'New installations',
      installations: 'Installations',
    } satisfies Record<SettingsSection, string>,
    sectionHints: {
      general: 'Appearance, the launcher window while you play, and where data is kept.',
      java: 'Which Java runtimes the launcher downloads for new installations.',
      defaults: 'Memory and window size given to each new installation.',
      installations: 'Memory and window size of an existing installation.',
    } satisfies Record<SettingsSection, string>,
    javaProvider: 'Java runtime source',
    javaProviders: {
      mojang: 'Mojang (recommended)',
      temurin: 'Eclipse Temurin (Adoptium)',
    } satisfies Record<JavaProvider, string>,
    javaProviderHints: {
      mojang:
        'The runtimes the official Minecraft launcher uses, with the exact Java each version asks for. Where Mojang publishes none for this computer, Temurin is used.',
      temurin: 'Current Eclipse Temurin builds of the Java version each Minecraft version needs.',
    } satisfies Record<JavaProvider, string>,
    javaApplies:
      'Applies to new installations. An installed version keeps the Java it was installed with, and Repair restores exactly that runtime.',
    javaInstalled: 'Java of each installation',
    javaShort: {
      mojang: 'Mojang',
      temurin: 'Temurin',
    } satisfies Record<JavaProvider, string>,
    javaValue: (provider: string, major: number, release: string) =>
      `${provider} · Java ${String(major)} (${release})`,
    noInstallations: 'No installations yet.',
    installation: 'Installation',
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
    dataLocationHint: 'Chosen by the operating system for this user. It cannot be changed here.',
    defaultsTitle: 'Defaults for new installations',
    defaultsSave: 'Save defaults',
    defaultsSaved: 'Saved.',
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
      stopped: 'Stopped by you',
    } satisfies Record<RunState, string>,
    exitCode: 'Exit code',
    duration: 'Duration',
    seconds: (ms: number) => `${(ms / 1000).toFixed(1)} s`,
    drainTimedOut:
      'Output capture was closed 2 seconds after exit because another process kept the output open.',
    outputTitle: 'Last output lines',
    dropped: (n: number) => `${String(n)} earlier lines not shown.`,
    stop: 'Stop',
    stopGame: 'Stop game',
    notStarted: 'Not started',
    stopWarning:
      'Stopping ends Minecraft immediately. Anything not saved in the current world is lost. Quit from the game menu to save.',
    stopConfirm: 'Stop without saving',
    keepPlaying: 'Keep playing',
    loggingDegraded:
      'The output log could not be written to disk; the last lines are still shown here.',
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
    jobTitle: 'Game files are still being prepared',
    jobBody:
      'Quitting now cancels the installation or repair; files verified so far are kept for next time. Keep it running in the background and the launcher window returns when it finishes.',
    jobStop: 'Cancel and quit',
  },

  activity: {
    install: 'Installing',
    repair: 'Checking files',
    launch: 'Playing',
    percent: (n: number) => `${String(n)}%`,
  },

  diagnostics: {
    title: 'Diagnostics',
    recentTitle: 'Recent operations',
    columns: {
      operation: 'Operation',
      installation: 'Installation',
      result: 'Result',
      transferred: 'Downloaded',
    },
    none: 'No operations yet.',
    kind: { install: 'Install', repair: 'Repair', launch: 'Launch' },
    outcome: {
      succeeded: 'Succeeded',
      failed: 'Failed',
      cancelled: 'Cancelled',
      interrupted: 'Interrupted',
    },
    exportTitle: 'Export diagnostics',
    exportExplanation:
      'Creates a ZIP file you can attach to a support request. It contains launcher and system information, installation versions and health, recent operation results and the launcher-captured output of the last three runs. Home and launcher folder paths are shortened and sign-in tokens are removed.',
    exportExcluded:
      'Never included: account records, sign-in tokens, launch argument files, worlds, screenshots, options or other files from game folders.',
    exportButton: 'Export…',
    exported: (name: string) => `Saved ${name}. Review it before sharing.`,
    exportCancelled: 'Export cancelled.',
  },

  about: {
    title: 'About',
    sourceCode: 'Source code',
    license:
      'License: not chosen yet. The project owner has not published a license for this source code.',
    officialTitle: 'Official Minecraft information',
    eula: 'Minecraft EULA',
    help: 'Minecraft Help',
  },

  foundation: {
    title: 'Project status',
    body: 'Installing, repairing and launching are implemented. Starting the game needs Microsoft sign-in, which this build cannot offer until the project supplies its own Microsoft application ID.',
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
