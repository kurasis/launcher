import type { UserData } from '../../lib/contracts';
import { strings } from '../../lib/strings';

const t = strings.deleteInstance;

/** Worlds, mods, packs or screenshots: worth a backup before deleting (D-055). */
export function hasUserData(data: UserData): boolean {
  return (
    data.worlds.length + data.mods + data.resourcePacks + data.shaderPacks + data.screenshots > 0
  );
}

/** "3 worlds (A, B, C), 12 mods, 5 screenshots" for what the folder holds. */
export function contents(data: UserData): string[] {
  const parts: string[] = [];
  if (data.worlds.length > 0) {
    parts.push(t.worlds({ count: data.worlds.length, names: data.worlds.join(', ') }));
  }
  if (data.mods > 0) parts.push(t.mods({ count: data.mods }));
  if (data.resourcePacks > 0) parts.push(t.resourcePacks({ count: data.resourcePacks }));
  if (data.shaderPacks > 0) parts.push(t.shaderPacks({ count: data.shaderPacks }));
  if (data.screenshots > 0) parts.push(t.screenshots({ count: data.screenshots }));
  return parts;
}
