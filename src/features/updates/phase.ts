import type { UpdateView } from '../../lib/contracts';
import { strings } from '../../lib/strings';

const t = strings.updates.settings.phases;

/** Where the update stands, as a sentence; null when there is nothing to say. */
export function phaseText(update: UpdateView): string | null {
  const version = update.version;
  if (update.phase === 'failed') return t.failed;
  if (update.phase === 'idle' || version === null) return null;
  switch (update.phase) {
    case 'available':
    case 'notice':
      return t.available({ version });
    case 'downloading':
      return t.downloading({ version });
    case 'ready':
      return t.ready({ version });
  }
}
