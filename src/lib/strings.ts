// User-facing English strings. Kept in one module so a later localization pass can
// replace this object without touching components.
import type { BuildProfile, MicrosoftClientIdStatus } from './contracts';

export const strings = {
  productName: 'Kurasis Launcher',
  buildTag: 'Development build',
  independentNotice:
    'Kurasis Launcher is an independent project. It is not affiliated with or endorsed by Mojang Studios or Microsoft.',

  foundation: {
    title: 'Project foundation',
    body: 'This build contains the desktop shell only. Sign-in, game installation and launching are not implemented yet, so no play controls are shown.',
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
