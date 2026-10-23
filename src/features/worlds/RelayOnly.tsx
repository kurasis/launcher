import { useState } from 'react';
import { Notice } from '../../components/Notice';
import { Panel } from '../../components/Panel';
import { Switch } from '../../components/Switch';
import type { SettingsView } from '../../lib/contracts';
import { ipc } from '../../lib/ipc';
import { strings } from '../../lib/strings';
import { worldsErrorText } from './useWorlds';

interface Props {
  settings: SettingsView;
}

/**
 * The relay-only privacy mode (W10): worlds connect through relay.lapi.sh
 * only, so other players never learn this computer's address. What the relay
 * still sees is said next to it.
 */
export function RelayOnly({ settings }: Props) {
  const t = strings.worlds.privacy;
  const [busy, setBusy] = useState(false);
  const [error, setError] = useState<string | null>(null);

  return (
    <Panel title={t.title} level={3}>
      <Switch
        label={t.relayOnly}
        hint={t.relayOnlyHint}
        checked={settings.worldsRelayOnly}
        disabled={!settings.writable}
        pending={busy}
        onChange={(on) => {
          setBusy(true);
          setError(null);
          ipc
            .setAppSettings({ worldsRelayOnly: on })
            .catch((e: unknown) => {
              setError(worldsErrorText(e));
            })
            .finally(() => {
              setBusy(false);
            });
        }}
      />
      {error !== null && <Notice tone="danger">{error}</Notice>}
    </Panel>
  );
}
