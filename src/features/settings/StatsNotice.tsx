import { useState } from 'react';
import { Button } from '../../components/Button';
import { Notice } from '../../components/Notice';
import type { SettingsView } from '../../lib/contracts';
import { ipc } from '../../lib/ipc';
import { strings } from '../../lib/strings';
import styles from './StatsNotice.module.css';

const t = strings.privacy.notice;

/**
 * The line about the anonymous statistics (D-071), under the feed (or the
 * installations when the feed is off): shown until Got it or Settings, at the
 * first start and once after the update that brought the statistics.
 */
export function StatsNotice({
  settings,
  onSettings,
}: {
  settings: SettingsView;
  onSettings: () => void;
}) {
  const [read, setRead] = useState(false);
  if (read || !settings.stats || settings.statsNoticeSeen || !settings.writable) return null;
  const seen = () => {
    setRead(true);
    // Only a convenience: a failure shows the line again at the next start.
    void ipc.setAppSettings({ statsNoticeSeen: true }).catch(() => undefined);
  };
  return (
    <Notice tone="info">
      <div className={styles.row}>
        <p>{t.text}</p>
        <div className={styles.actions}>
          <Button onClick={seen}>{t.gotIt}</Button>
          <Button
            aria-label={t.settingsLabel}
            onClick={() => {
              seen();
              onSettings();
            }}
          >
            {t.settings}
          </Button>
        </div>
      </div>
    </Notice>
  );
}
