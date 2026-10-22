import { useState } from 'react';
import { Button } from '../../components/Button';
import { Notice } from '../../components/Notice';
import type { SettingsPatch, SettingsView, StatsSent } from '../../lib/contracts';
import { describeIpcError, ipc } from '../../lib/ipc';
import { strings } from '../../lib/strings';
import { useCommand } from '../../lib/useCommand';
import network from '../network/Network.module.css';
import styles from './SettingsView.module.css';

const t = strings.privacy;

/** One request as it goes, word for word. */
function Packet({ title, value }: { title: string; value: unknown }) {
  return (
    <div className={styles.field}>
      <span>{title}</span>
      <pre className={styles.packet} dir="ltr">
        {JSON.stringify(value, null, 2)}
      </pre>
    </div>
  );
}

/** Settings → Privacy → What is sent (D-071): the last ping and ad counts as
 * they went, and today's ping before it goes. */
function WhatIsSent({ sent, on }: { sent: StatsSent; on: boolean }) {
  const nothing = sent.next === null && sent.ping === null && sent.counts === null;
  return (
    <div className={styles.section}>
      <p className={styles.hint}>{t.sentIntro}</p>
      {sent.next !== null && <Packet title={t.sentNext} value={sent.next} />}
      {sent.ping !== null && <Packet title={t.sentPing} value={sent.ping} />}
      {sent.counts !== null && <Packet title={t.sentCounts} value={sent.counts} />}
      {nothing && <p>{on ? t.sentNothing : t.sentOff}</p>}
      {sent.waiting > 0 && <p className={styles.hint}>{t.sentWaiting({ count: sent.waiting })}</p>}
    </div>
  );
}

/** What goes to the developers and with which consent (D-061, D-071). */
export function PrivacySettings({ settings }: { settings: SettingsView }) {
  const update = useCommand((requestId: string, patch: SettingsPatch) =>
    ipc.setAppSettings(patch, requestId),
  );
  const disabled = update.pending || !settings.writable;
  const [open, setOpen] = useState(false);
  const [sent, setSent] = useState<StatsSent | null>(null);
  const [sentError, setSentError] = useState<string | null>(null);

  const load = () => {
    setSentError(null);
    void ipc
      .statsSent()
      .then(setSent)
      .catch((failure: unknown) => {
        setSentError(describeIpcError(failure));
      });
  };

  return (
    <div className={styles.section}>
      <label className={network.check}>
        <input
          type="checkbox"
          checked={settings.stats}
          disabled={disabled}
          onChange={(event) => {
            const on = event.target.checked;
            void update.run({ stats: on, statsNoticeSeen: true }).then(() => {
              if (open) load();
            });
          }}
        />
        <span>
          <span className={network.strong}>{t.stats}</span>
          <span className={network.hint}>{t.statsHint}</span>
        </span>
      </label>
      <div>
        <Button
          aria-expanded={open}
          onClick={() => {
            if (!open) load();
            setOpen(!open);
          }}
        >
          {t.whatIsSent}
        </Button>
      </div>
      {open && sentError !== null && <Notice tone="danger">{sentError}</Notice>}
      {open && sentError === null && sent === null && <p role="status">{t.sentLoading}</p>}
      {open && sent !== null && <WhatIsSent sent={sent} on={settings.stats} />}
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
