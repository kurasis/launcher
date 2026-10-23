import { useState } from 'react';
import { Button } from '../../components/Button';
import { Notice } from '../../components/Notice';
import { Panel } from '../../components/Panel';
import { Switch } from '../../components/Switch';
import type { SettingsPatch, SettingsView, StatsSent } from '../../lib/contracts';
import { describeIpcError, ipc } from '../../lib/ipc';
import { strings } from '../../lib/strings';
import { useCommand } from '../../lib/useCommand';
import styles from './SettingsView.module.css';

const t = strings.privacy;

/** One request as it goes, word for word. */
function Packet({ title, value }: { title: string; value: unknown }) {
  return (
    <div className={styles.block}>
      <span className={styles.label}>{title}</span>
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
    <>
      <p className={styles.hint}>{t.sentIntro}</p>
      {sent.next !== null && <Packet title={t.sentNext} value={sent.next} />}
      {sent.ping !== null && <Packet title={t.sentPing} value={sent.ping} />}
      {sent.counts !== null && <Packet title={t.sentCounts} value={sent.counts} />}
      {nothing && <p>{on ? t.sentNothing : t.sentOff}</p>}
      {sent.waiting > 0 && <p className={styles.hint}>{t.sentWaiting({ count: sent.waiting })}</p>}
    </>
  );
}

/** What goes to the developers and with which consent (D-061, D-071). */
export function PrivacySettings({ settings }: { settings: SettingsView }) {
  const update = useCommand((requestId: string, patch: SettingsPatch) =>
    ipc.setAppSettings(patch, requestId),
  );
  const renew = useCommand((requestId: string) =>
    ipc.setAppSettings({ newInstallId: true }, requestId),
  );
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
    <>
      <Panel>
        <Switch
          label={t.stats}
          hint={t.statsHint}
          checked={settings.stats}
          disabled={!settings.writable}
          pending={update.pending}
          onChange={(on) => {
            void update.run({ stats: on, statsNoticeSeen: true }).then(() => {
              if (open) load();
            });
          }}
        />
        <div className={styles.actions}>
          <Button
            size="small"
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
        {open && sentError === null && sent === null && (
          <p className={styles.hint} role="status">
            {t.sentLoading}
          </p>
        )}
        {open && sent !== null && <WhatIsSent sent={sent} on={settings.stats} />}
        {update.error !== null && <Notice tone="danger">{update.error}</Notice>}
      </Panel>
      <Panel title={t.installId} level={3}>
        <p>{t.reports}</p>
        {settings.installId === null ? (
          <p className={styles.hint}>{t.installIdNone}</p>
        ) : (
          <>
            <code className={styles.path} dir="ltr">
              {settings.installId}
            </code>
            <p className={styles.hint}>{t.newInstallIdHint}</p>
            <div className={styles.actions}>
              <Button
                size="small"
                disabled={!settings.writable}
                aria-disabled={renew.pending || undefined}
                onClick={() => {
                  if (!renew.pending) void renew.run();
                }}
              >
                {t.newInstallId}
              </Button>
            </div>
          </>
        )}
        {renew.error !== null && <Notice tone="danger">{renew.error}</Notice>}
      </Panel>
    </>
  );
}
