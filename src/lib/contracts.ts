// Frontend mirror of the Rust DTOs in src-tauri/src/commands/. Field names use camelCase
// because the Rust structs are serialized with `rename_all = "camelCase"`.
// Keep both sides in sync; P02 introduces the versioned AppSnapshot contract.

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
