import type { LoaderChoice, LoaderKind } from '../../lib/contracts';
import { strings } from '../../lib/strings';

/** Loaders in the order the picker offers them (D-039). */
export const LOADER_KINDS: LoaderKind[] = ['forge', 'fabric', 'quilt', 'neoforge', 'optifine'];

/** The loaders that run mods: OptiFine on its own runs none. */
export const MOD_LOADER_KINDS: LoaderKind[] = LOADER_KINDS.filter((kind) => kind !== 'optifine');

/**
 * A loader version as people call it: Forge's `1.21.1-52.1.0` is `52.1.0`,
 * OptiFine's `1.21.1_HD_U_J1` is `HD U J1`.
 */
export function loaderVersionLabel(loader: LoaderChoice, gameVersion: string): string {
  if (loader.kind === 'optifine') {
    const prefix = `${gameVersion}_`;
    const edition = loader.version.startsWith(prefix)
      ? loader.version.slice(prefix.length)
      : loader.version;
    return edition.replaceAll('_', ' ');
  }
  const prefix = `${gameVersion}-`;
  return loader.version.startsWith(prefix) ? loader.version.slice(prefix.length) : loader.version;
}

/** "Minecraft 1.21.1", or "Minecraft 1.21.1 · Fabric 0.16.14" with a loader. */
export function gameLabel(instance: { versionId: string; loader: LoaderChoice | null }): string {
  const t = strings.loaders;
  const game = t.game({ version: instance.versionId });
  if (instance.loader === null) return game;
  return t.gameWithLoader({
    game,
    loader: t.names[instance.loader.kind],
    version: loaderVersionLabel(instance.loader, instance.versionId),
  });
}
