import type { InstanceSummary } from '../../lib/contracts';

/**
 * The installation of this game version a version's Play starts: vanilla
 * before one with a loader, ready before not, then the one played last.
 */
export function installationOf(
  instances: InstanceSummary[],
  version: string,
): InstanceSummary | null {
  const rank = (instance: InstanceSummary) =>
    (instance.health === 'ready' ? 0 : 2) + (instance.loader === null ? 0 : 1);
  let found: InstanceSummary | null = null;
  for (const instance of instances) {
    if (instance.versionId !== version) continue;
    if (
      found === null ||
      rank(instance) < rank(found) ||
      (rank(instance) === rank(found) && (instance.lastPlayedAt ?? '') > (found.lastPlayedAt ?? ''))
    ) {
      found = instance;
    }
  }
  return found;
}
