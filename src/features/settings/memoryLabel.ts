import { useEffect, useState } from 'react';
import type { MemoryRecommendation, Workload } from '../../lib/contracts';
import { ipc } from '../../lib/ipc';
import { strings } from '../../lib/strings';

const t = strings.memory;

/** "Minecraft without mods", "Mod loader with 330 mods", ... */
export function workloadLabel(workload: Workload): string {
  switch (workload.kind) {
    case 'legacy':
      return t.workload.legacy;
    case 'vanilla':
      return t.workload.vanilla;
    case 'modded':
      return t.workload.modded({ mods: workload.mods });
  }
}

/** The recommendation sentence, mentioning when this computer limits it. */
export function recommendationText(recommendation: MemoryRecommendation): string {
  const reason = workloadLabel(recommendation.workload);
  return recommendation.wantedMib === null
    ? t.recommended({ mib: recommendation.mib, reason })
    : t.limited({ mib: recommendation.mib, wanted: recommendation.wantedMib, reason });
}

/** Asks the core for a recommendation; null until it answers or when it cannot. */
export function useRecommendation(
  target: { instanceId: string } | { versionId: string; modded: boolean } | null,
): MemoryRecommendation | null {
  const [found, setFound] = useState<{ key: string; value: MemoryRecommendation } | null>(null);
  const key = target === null ? '' : JSON.stringify(target);
  useEffect(() => {
    if (key === '') return;
    let current = true;
    ipc
      .recommendMemory(JSON.parse(key) as Parameters<typeof ipc.recommendMemory>[0])
      .then((value) => {
        if (current) setFound({ key, value });
      })
      .catch(() => {
        // Without a recommendation the form keeps the machine-wide hint.
      });
    return () => {
      current = false;
    };
  }, [key]);
  return found?.key === key ? found.value : null;
}
