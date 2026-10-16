// The single typed entry point for Tauri IPC. UI code must not call `invoke` directly,
// so every command name and payload shape is declared in one place.
import { invoke } from '@tauri-apps/api/core';
import type { AppInfo } from './contracts';

export const ipc = {
  getAppInfo: (): Promise<AppInfo> => invoke<AppInfo>('get_app_info'),
};

/** Converts an unknown IPC rejection into a message that is safe to show. */
export function describeIpcError(error: unknown): string {
  if (typeof error === 'string' && error.length > 0) {
    return error;
  }
  if (error instanceof Error && error.message.length > 0) {
    return error.message;
  }
  return 'The launcher core did not respond.';
}
