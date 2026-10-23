import type { TrafficUsage } from './contracts';

/** Everything the launcher downloaded in a month, from all sources (D-051). */
export function trafficTotal(usage: TrafficUsage): number {
  return usage.game + usage.java + usage.loaders + usage.mods + usage.metadata;
}
