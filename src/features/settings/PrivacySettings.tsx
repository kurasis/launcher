import { Button } from '../../components/Button';
import { Notice } from '../../components/Notice';
import type { SettingsPatch, SettingsView } from '../../lib/contracts';
import { ipc } from '../../lib/ipc';
import { strings } from '../../lib/strings';
import { useCommand } from '../../lib/useCommand';
import network from '../network/Network.module.css';
import styles from './SettingsView.module.css';

const t = strings.privacy;

/** What goes to the developers and with which consent (D-061). */
export function PrivacySettings({ settings }: { settings: SettingsView }) {
  const update = useCommand((requestId: string, patch: SettingsPatch) =>
    ipc.setAppSettings(patch, requestId),
  );
  const disabled = update.pending || !settings.writable;

  return (
    <div className={styles.section}>
      <label className={network.check}>
        <input
          type="checkbox"
          checked={settings.shareStats}
          disabled={disabled}
          onChange={(event) => void update.run({ shareStats: event.target.checked })}
        />
        <span>
          <span className={network.strong}>{t.shareStats}</span>
          <span className={network.hint}>{t.shareStatsHint}</span>
        </span>
      </label>
      <p>{t.reports}</p>
      <div className={styles.field}>
        <span className={network.strong}>{t.installId}</span>
        {settings.installId === null ? (
          <span className={styles.hint}>{t.installIdNone}</span>
        ) : (
          <>
            <code className={styles.path}>{settings.installId}</code>
            <span className={styles.hint}>{t.newInstallIdHint}</span>
            <div>
              <Button disabled={disabled} onClick={() => void update.run({ newInstallId: true })}>
                {t.newInstallId}
              </Button>
            </div>
          </>
        )}
      </div>
      {update.error !== null && <Notice tone="danger">{update.error}</Notice>}
    </div>
  );
}
