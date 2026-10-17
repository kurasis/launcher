import type { LoaderChoice, LoaderKind } from '../../lib/contracts';
import { strings } from '../../lib/strings';

/** Loaders in the order the picker offers them (D-039). */
export const LOADER_KINDS: LoaderKind[] = ['forge', 'fabric', 'quilt', 'neoforge'];

/** A loader version as people call it: Forge's `1.21.1-52.1.0` is `52.1.0`. */
export function loaderVersionLabel(loader: LoaderChoice, gameVersion: string): string {
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
