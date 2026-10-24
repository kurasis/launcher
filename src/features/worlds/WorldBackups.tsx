import { useCallback, useEffect, useRef, useState } from 'react';
import { Button } from '../../components/Button';
import { Confirm } from '../../components/Confirm';
import { Notice } from '../../components/Notice';
import { Panel } from '../../components/Panel';
import { useFocusBack } from '../../components/useFocusBack';
import type { BackupInfo, LocalWorld, WorldsState } from '../../lib/contracts';
import { formatBytes } from '../../lib/format';
import { ipc } from '../../lib/ipc';
import { numberLocale, strings } from '../../lib/strings';
import { shortWhen } from '../../lib/time';
import { worldsErrorText } from './useWorlds';
import styles from './Worlds.module.css';

/** Automatic backups kept per world (`KEEP_AFTER_PLAY`). */
const KEPT = 10;

interface Props {
  world: LocalWorld;
  state: WorldsState;
}

/** The full date and time, for the restore question. */
const when = (backup: BackupInfo) => new Date(backup.createdAt).toLocaleString(numberLocale());

/** "Today, 19:55", "Yesterday, 23:10", "3 October, 18:02": the time kept on older days,
 * as one day can have several backups. */
const backupWhen = (backup: BackupInfo) =>
  shortWhen(backup.createdAt, { time: 'always', capital: true });

/**
 * A hosted world's backups on this computer (W9): one after each session
 * that changed it, one on request, a restore that backs up first, and an
 * export as zip. All of it waits while the world is open.
 */
export function WorldBackups({ world, state }: Props) {
  const t = strings.worlds.safety;
  const b = strings.backups;
  const [backups, setBackups] = useState<BackupInfo[] | null>(null);
  const [error, setError] = useState<string | null>(null);
  const [note, setNote] = useState<string | null>(null);
  const [busy, setBusy] = useState(false);
  const [restoring, setRestoring] = useState<BackupInfo | null>(null);
  // The Restore that asked: the focus goes back to it when the question closes.
  const opener = useRef<HTMLButtonElement | null>(null);
  const list = useRef<HTMLUListElement>(null);
  useFocusBack(restoring?.id ?? null, opener, list);

  const hosted = state.hosting?.world === world.id && state.hosting.clean === null;
  // Waits something else set stay disabled; a backup, restore or export under way (its
  // own press sets `maintenance`) keeps the focus on the pressed button.
  const waiting = hosted || state.preparing !== null;
  const working = busy || state.maintenance;

  const load = useCallback(() => {
    ipc
      .worldsBackups(world.id)
      .then(setBackups)
      .catch((e: unknown) => {
        setBackups((current) => current ?? []);
        setError(worldsErrorText(e));
      });
  }, [world.id]);

  // Again when a session's backup or another change has just finished.
  useEffect(() => {
    if (!state.maintenance) load();
  }, [load, state.maintenance]);

  const run = (action: () => Promise<string | null>) => {
    if (working || waiting) return;
    setBusy(true);
    setError(null);
    setNote(null);
    action()
      .then((said) => {
        setNote(said);
        load();
      })
      .catch((e: unknown) => {
        setError(worldsErrorText(e));
      })
      .finally(() => {
        setBusy(false);
      });
  };

  const played = world.played || (backups?.length ?? 0) > 0;
  const pending = working || undefined;

  return (
    <Panel title={t.title} level={3}>
      <p className={styles.panelHint}>{t.hint({ count: KEPT })}</p>
      {!played ? (
        <p className={styles.panelHint}>{t.notPlayed}</p>
      ) : (
        <div className={styles.actions}>
          <Button
            size="small"
            disabled={waiting}
            aria-disabled={pending}
            onClick={() => {
              run(async () => {
                await ipc.worldsBackUp(world.id);
                return t.created;
              });
            }}
          >
            {t.backUp}
          </Button>
          <Button
            size="small"
            disabled={waiting}
            aria-disabled={pending}
            onClick={() => {
              run(async () => {
                const saved = await ipc.worldsExport(world.id, t.exportTitle);
                return saved.saved ? t.exported({ name: saved.fileName ?? '' }) : null;
              });
            }}
          >
            {t.export}
          </Button>
        </div>
      )}
      {hosted && <p className={styles.panelHint}>{t.whileHosted}</p>}
      <p className={styles.live} role="status">
        {working ? b.working : (note ?? '')}
      </p>
      {error !== null && <Notice tone="danger">{error}</Notice>}
      {backups !== null && backups.length > 0 && (
        <ul ref={list} className={styles.backupList} aria-label={t.list} tabIndex={-1}>
          {backups.map((backup) => (
            <li key={backup.id} className={styles.backupRow}>
              <span className={styles.backupWhen}>{backupWhen(backup)}</span>
              <span className={styles.rowMeta}>
                {t.row({ reason: b.reason[backup.reason], size: formatBytes(backup.totalBytes) })}
              </span>
              <div className={styles.rowActions}>
                <Button
                  size="small"
                  disabled={waiting}
                  aria-disabled={pending}
                  onClick={(event) => {
                    if (working) return;
                    opener.current = event.currentTarget;
                    setRestoring(backup);
                  }}
                >
                  {b.restore}
                </Button>
              </div>
              {restoring?.id === backup.id && (
                <div className={styles.rowWide}>
                  <Confirm
                    label={world.name}
                    text={t.confirmRestore({ date: when(backup), world: world.name })}
                    action={b.restore}
                    actionVariant="primary"
                    cancel={strings.worlds.cancel}
                    pending={working}
                    onYes={() => {
                      setRestoring(null);
                      run(async () => {
                        await ipc.worldsRestore(world.id, backup.id);
                        return t.restored({ date: when(backup) });
                      });
                    }}
                    onNo={() => {
                      setRestoring(null);
                    }}
                  />
                </div>
              )}
            </li>
          ))}
        </ul>
      )}
    </Panel>
  );
}
