import { useState } from 'react';
import { Notice } from '../../components/Notice';
import forms from '../../components/forms.module.css';
import type { SettingsView } from '../../lib/contracts';
import { ipc } from '../../lib/ipc';
import { strings } from '../../lib/strings';
import { worldsErrorText } from './useWorlds';
import styles from './Worlds.module.css';

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
    <section className={styles.section} aria-label={t.title}>
      <h3 className={styles.sectionTitle}>{t.title}</h3>
      <label className={forms.choice}>
        <input
          type="checkbox"
          checked={settings.worldsRelayOnly}
          disabled={busy || !settings.writable}
          onChange={(event) => {
            const on = event.target.checked;
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
        {t.relayOnly}
      </label>
      <p className={styles.muted}>{t.relayOnlyHint}</p>
      {error !== null && <Notice tone="danger">{error}</Notice>}
    </section>
  );
}
