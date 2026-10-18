import { useEffect, useState } from 'react';
import { Button } from '../../components/Button';
import { Notice } from '../../components/Notice';
import forms from '../../components/forms.module.css';
import type { DataLocation, LocationTarget } from '../../lib/contracts';
import { describeIpcError, ipc } from '../../lib/ipc';
import { strings } from '../../lib/strings';
import { useCommand } from '../../lib/useCommand';
import styles from './SettingsView.module.css';

const t = strings.settings.dataFolder;

/**
 * Where the launcher keeps its data (D-043): shows the folder and moves the data
 * to another folder, back to the default, or beside the launcher (portable).
 */
export function DataFolderSettings({ busy }: { busy: boolean }) {
  const [location, setLocation] = useState<DataLocation | null>(null);
  const [loadError, setLoadError] = useState<string | null>(null);
  const [confirm, setConfirm] = useState<{ target: LocationTarget; path: string } | null>(null);
  const move = useCommand((requestId: string, target: LocationTarget, path: string | null) =>
    ipc.moveDataLocation(target, path, requestId),
  );

  useEffect(() => {
    ipc
      .getDataLocation()
      .then(setLocation)
      .catch((error: unknown) => {
        setLoadError(describeIpcError(error));
      });
  }, []);

  if (location === null) {
    return loadError === null ? null : <Notice tone="danger">{loadError}</Notice>;
  }
  const pending = location.pendingPath !== null;
  const disabled = busy || move.pending || pending;

  const run = async (target: LocationTarget, path: string) => {
    setConfirm(null);
    const moved = await move.run(target, target === 'custom' ? path : null);
    if (moved) setLocation(moved);
  };

  return (
    <div>
      <h3 className={styles.subheading}>{strings.settings.dataLocation}</h3>
      <p className={styles.path} dir="ltr">
        {location.path}
      </p>
      <p className={styles.hint}>{t.modes[location.mode]}</p>
      {pending ? (
        <Notice tone="info">
          {location.pendingReused ? t.pendingReused : t.pending}{' '}
          <span dir="ltr" className={styles.path}>
            {location.pendingPath}
          </span>
        </Notice>
      ) : confirm !== null ? (
        <Notice tone="warning">
          <p>
            {t.confirm}{' '}
            <span dir="ltr" className={styles.path}>
              {confirm.path}
            </span>
          </p>
          <p>{t.confirmHint}</p>
          <div className={forms.actions}>
            <Button variant="primary" onClick={() => void run(confirm.target, confirm.path)}>
              {t.copy}
            </Button>
            <Button
              onClick={() => {
                setConfirm(null);
              }}
            >
              {strings.createForm.cancel}
            </Button>
          </div>
        </Notice>
      ) : (
        <div className={forms.actions}>
          <Button
            disabled={disabled || location.mode === 'portable'}
            onClick={() => {
              void ipc
                .pickDataFolder()
                .then((path) => {
                  if (path !== null) setConfirm({ target: 'custom', path });
                })
                .catch(() => undefined);
            }}
          >
            {t.choose}
          </Button>
          {location.mode !== 'default' && (
            <Button
              disabled={disabled}
              onClick={() => {
                setConfirm({ target: 'default', path: location.defaultPath });
              }}
            >
              {t.toDefault}
            </Button>
          )}
          {location.mode !== 'portable' && location.portablePath !== null && (
            <Button
              disabled={disabled}
              onClick={() => {
                if (location.portablePath !== null) {
                  setConfirm({ target: 'portable', path: location.portablePath });
                }
              }}
            >
              {t.toPortable}
            </Button>
          )}
        </div>
      )}
      {move.pending && <p role="status">{t.copying}</p>}
      {busy && !pending && <p className={styles.hint}>{t.busy}</p>}
      {move.error !== null && <Notice tone="danger">{move.error}</Notice>}
      <p className={styles.hint}>{t.portableHint}</p>
    </div>
  );
}
