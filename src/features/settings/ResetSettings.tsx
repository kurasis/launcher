import { useCallback, useEffect, useState } from 'react';
import { Button } from '../../components/Button';
import { Notice } from '../../components/Notice';
import forms from '../../components/forms.module.css';
import type { ResetChoice, ResetPreview, ResetReport } from '../../lib/contracts';
import { formatBytes } from '../../lib/format';
import { describeIpcError, ipc } from '../../lib/ipc';
import { strings } from '../../lib/strings';
import { useCommand } from '../../lib/useCommand';
import { contents, hasUserData } from '../instances/userData';
import styles from './SettingsView.module.css';

const t = strings.settings.reset;

type Part = Exclude<keyof ResetChoice, 'backUpInstallations'>;
const PARTS: Part[] = [
  'installations',
  'gameFiles',
  'backups',
  'accounts',
  'settings',
  'traffic',
  'temporary',
];

const NOTHING: ResetChoice = {
  settings: false,
  installations: false,
  backUpInstallations: true,
  gameFiles: false,
  accounts: false,
  backups: false,
  traffic: false,
  temporary: false,
};

function describe(part: Part, preview: ResetPreview): string {
  switch (part) {
    case 'installations':
      return t.hints.installations({ count: preview.installations.length });
    case 'gameFiles':
      return t.hints.gameFiles({ size: formatBytes(preview.gameFilesBytes) });
    case 'backups':
      return t.hints.backups({
        count: preview.backupCount,
        size: formatBytes(preview.backupBytes),
      });
    case 'accounts':
      return t.hints.accounts({
        microsoft: preview.microsoftAccounts,
        profiles: preview.notokenProfiles,
      });
    case 'temporary':
      return t.hints.temporary({ size: formatBytes(preview.temporaryBytes) });
    case 'settings':
      return t.hints.settings;
    case 'traffic':
      return t.hints.traffic;
  }
}

function summary(report: ResetReport): string {
  return t.done({
    installations: report.deletedInstallations,
    backedUp: report.backedUp,
    backups: report.deletedBackups,
    accounts: report.signedOut,
    size: formatBytes(report.freedBytes),
  });
}

/**
 * Settings → Reset (D-057): choose what to put back to its first-start state.
 * Installations holding worlds, mods, packs or screenshots are listed, with a
 * backup of them offered first (kept even when old backups are deleted).
 */
export function ResetSettings({ busy }: { busy: boolean }) {
  const [preview, setPreview] = useState<ResetPreview | null>(null);
  const [loadError, setLoadError] = useState<string | null>(null);
  const [choice, setChoice] = useState<ResetChoice>(NOTHING);
  const [confirming, setConfirming] = useState(false);
  const [message, setMessage] = useState<string | null>(null);
  const reset = useCommand((requestId: string, chosen: ResetChoice) =>
    ipc.resetLauncher(chosen, requestId),
  );

  const reload = useCallback(() => {
    ipc
      .resetPreview()
      .then((found) => {
        setPreview(found);
        setLoadError(null);
      })
      .catch((error: unknown) => {
        setLoadError(describeIpcError(error));
      });
  }, []);
  useEffect(reload, [reload]);

  const chosen = PARTS.filter((part) => choice[part]);
  const withData = preview?.installations.filter((i) => hasUserData(i.data)) ?? [];
  const disabled = busy || reset.pending;

  const toggle = (part: Part, on: boolean) => {
    setChoice((current) => ({ ...current, [part]: on }));
    setConfirming(false);
    setMessage(null);
  };

  return (
    <div className={styles.section}>
      {loadError !== null && <Notice tone="danger">{loadError}</Notice>}
      {busy && <Notice tone="info">{t.busy}</Notice>}
      {preview !== null && (
        <fieldset className={forms.choices} disabled={disabled}>
          <legend>{t.choose}</legend>
          {PARTS.map((part) => (
            <label key={part} className={styles.option}>
              <input
                type="checkbox"
                checked={choice[part]}
                onChange={(event) => {
                  toggle(part, event.target.checked);
                }}
              />
              <span>
                <span className={styles.optionTitle}>{t.parts[part]}</span>
                <span className={styles.hint}>{describe(part, preview)}</span>
              </span>
            </label>
          ))}
        </fieldset>
      )}
      {choice.installations && withData.length > 0 && (
        <Notice tone="warning">
          <p>{t.userData}</p>
          <ul>
            {withData.map((item) => (
              <li key={item.instanceId}>
                <strong>{item.displayName}</strong> ({item.versionLabel}):{' '}
                {contents(item.data).join(', ')}
              </li>
            ))}
          </ul>
          <label className={forms.choice}>
            <input
              type="checkbox"
              checked={choice.backUpInstallations}
              disabled={disabled}
              onChange={(event) => {
                setChoice((current) => ({
                  ...current,
                  backUpInstallations: event.target.checked,
                }));
                setConfirming(false);
              }}
            />
            {t.backUpFirst}
          </label>
          {choice.backUpInstallations && choice.backups && <p>{t.backupsKept}</p>}
        </Notice>
      )}
      {confirming ? (
        <Notice tone="warning">
          <p>{t.confirm}</p>
          <ul>
            {chosen.map((part) => (
              <li key={part}>{t.parts[part]}</li>
            ))}
          </ul>
          <p>{t.final}</p>
          <div className={forms.actions}>
            <Button
              variant="primary"
              disabled={disabled}
              onClick={() => {
                void reset.run(choice).then((report) => {
                  setConfirming(false);
                  if (report === undefined) return;
                  setMessage(summary(report));
                  setChoice(NOTHING);
                  reload();
                });
              }}
            >
              {t.reset}
            </Button>
            <Button
              disabled={reset.pending}
              onClick={() => {
                setConfirming(false);
              }}
            >
              {t.cancel}
            </Button>
          </div>
        </Notice>
      ) : (
        <div className={forms.actions}>
          <Button
            disabled={disabled || chosen.length === 0}
            onClick={() => {
              setConfirming(true);
            }}
          >
            {t.start}
          </Button>
        </div>
      )}
      {reset.pending && <p role="status">{t.working}</p>}
      {reset.error !== null && <Notice tone="danger">{reset.error}</Notice>}
      {message !== null && <Notice tone="info">{message}</Notice>}
    </div>
  );
}
