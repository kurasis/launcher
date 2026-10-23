import { useEffect, useRef, useState } from 'react';
import { Button } from '../../components/Button';
import { Confirm } from '../../components/Confirm';
import { Notice } from '../../components/Notice';
import { Panel } from '../../components/Panel';
import { useFocusBack } from '../../components/useFocusBack';
import type { DataLocation, LocationTarget } from '../../lib/contracts';
import { describeIpcError, ipc } from '../../lib/ipc';
import { useShownError } from '../../lib/useShownError';
import { strings } from '../../lib/strings';
import { useCommand } from '../../lib/useCommand';
import styles from './SettingsView.module.css';

const t = strings.settings.dataFolder;

/**
 * Where the launcher keeps its data (D-043): shows the folder and moves the data
 * to another folder, back to the default, or beside the launcher (portable).
 */
export function DataFolderSettings({ busy }: { busy: boolean }) {
  return (
    <Panel title={strings.settings.dataLocation} level={3}>
      <DataFolder busy={busy} />
    </Panel>
  );
}

function DataFolder({ busy }: { busy: boolean }) {
  const [location, setLocation] = useState<DataLocation | null>(null);
  const [loadError, setLoadError] = useState<string | null>(null);
  const [confirm, setConfirm] = useState<{ target: LocationTarget; path: string } | null>(null);
  const picking = useShownError();
  const move = useCommand((requestId: string, target: LocationTarget, path: string | null) =>
    ipc.moveDataLocation(target, path, requestId),
  );
  // The button that asked, to take the focus back when the question closes; else the
  // line that says what happens now.
  const [asked, setAsked] = useState<LocationTarget>('custom');
  const choose = useRef<HTMLButtonElement>(null);
  const toDefault = useRef<HTMLButtonElement>(null);
  const toPortable = useRef<HTMLButtonElement>(null);
  const copying = useRef<HTMLParagraphElement>(null);
  const copied = useRef<HTMLDivElement>(null);
  const opener = { custom: choose, default: toDefault, portable: toPortable }[asked];
  const pending = location !== null && location.pendingPath !== null;
  useFocusBack(
    confirm !== null ? 'asking' : move.pending ? 'copying' : pending ? 'pending' : 'idle',
    opener,
    copying,
    copied,
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
    return loadError === null ? (
      <p className={styles.hint} role="status">
        {t.loading}
      </p>
    ) : (
      <Notice tone="danger">{loadError}</Notice>
    );
  }
  const disabled = busy || move.pending || pending;

  const ask = (target: LocationTarget, path: string) => {
    setAsked(target);
    setConfirm({ target, path });
  };
  const run = async (target: LocationTarget, path: string) => {
    setConfirm(null);
    const moved = await move.run(target, target === 'custom' ? path : null);
    if (moved) setLocation(moved);
  };

  return (
    <>
      <p>{t.modes[location.mode]}</p>
      <p className={styles.path} dir="ltr">
        {location.path}
      </p>
      {pending ? (
        <div ref={copied} className={styles.status} tabIndex={-1}>
          <Notice tone="info">
            {location.pendingReused ? t.pendingReused : t.pending}{' '}
            <span dir="ltr" className={styles.path}>
              {location.pendingPath}
            </span>
          </Notice>
        </div>
      ) : confirm !== null ? (
        <Confirm
          label={t.copy}
          action={t.copy}
          actionVariant="primary"
          cancel={strings.createForm.cancel}
          pending={move.pending}
          onYes={() => void run(confirm.target, confirm.path)}
          onNo={() => {
            setConfirm(null);
          }}
        >
          <p>
            {t.confirm}{' '}
            <span dir="ltr" className={styles.path}>
              {confirm.path}
            </span>
          </p>
          <p>{t.confirmHint}</p>
        </Confirm>
      ) : (
        <div className={styles.actions}>
          <Button
            ref={choose}
            size="small"
            disabled={disabled || location.mode === 'portable'}
            onClick={() => {
              picking.clear();
              void ipc
                .pickDataFolder(strings.windowTitles.pickDataFolder)
                .then((path) => {
                  if (path !== null) ask('custom', path);
                })
                .catch(picking.catcher);
            }}
          >
            {t.choose}
          </Button>
          {location.mode !== 'default' && (
            <Button
              ref={toDefault}
              size="small"
              disabled={disabled}
              onClick={() => {
                ask('default', location.defaultPath);
              }}
            >
              {t.toDefault}
            </Button>
          )}
          {location.mode !== 'portable' && location.portablePath !== null && (
            <Button
              ref={toPortable}
              size="small"
              disabled={disabled}
              onClick={() => {
                if (location.portablePath !== null) ask('portable', location.portablePath);
              }}
            >
              {t.toPortable}
            </Button>
          )}
        </div>
      )}
      {move.pending && (
        <p ref={copying} className={styles.status} role="status" tabIndex={-1}>
          {t.copying}
        </p>
      )}
      {busy && !pending && <p className={styles.hint}>{t.busy}</p>}
      {move.error !== null && <Notice tone="danger">{move.error}</Notice>}
      {picking.error !== null && <Notice tone="danger">{picking.error}</Notice>}
      <p className={styles.hint}>{t.portableHint}</p>
    </>
  );
}
