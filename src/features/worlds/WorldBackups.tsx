import { useCallback, useEffect, useState } from 'react';
import { Button } from '../../components/Button';
import { Notice } from '../../components/Notice';
import forms from '../../components/forms.module.css';
import type { BackupInfo, LocalWorld, WorldsState } from '../../lib/contracts';
import { formatBytes } from '../../lib/format';
import { ipc } from '../../lib/ipc';
import { numberLocale, strings } from '../../lib/strings';
import { Dialog } from '../../components/Dialog';
import { worldsErrorText } from './useWorlds';
import styles from './Worlds.module.css';

/** Automatic backups kept per world (`KEEP_AFTER_PLAY`). */
const KEPT = 10;

interface Props {
  world: LocalWorld;
  state: WorldsState;
}

const when = (backup: BackupInfo) => new Date(backup.createdAt).toLocaleString(numberLocale());

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

  const hosted = state.hosting?.world === world.id && state.hosting.clean === null;
  const waiting = hosted || state.maintenance || state.preparing !== null;

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

  return (
    <section className={styles.section} aria-label={t.title}>
      <h4 className={styles.sectionTitle}>{t.title}</h4>
      <p className={styles.muted}>{t.hint({ count: KEPT })}</p>
      {!played ? (
        <p className={styles.muted}>{t.notPlayed}</p>
      ) : (
        <div className={styles.actions}>
          <Button
            disabled={busy || waiting}
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
            disabled={busy || waiting}
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
      {hosted && <p className={styles.muted}>{t.whileHosted}</p>}
      {(busy || state.maintenance) && (
        <p className={styles.muted} role="status">
          {b.working}
        </p>
      )}
      {note !== null && (
        <p className={styles.muted} role="status">
          {note}
        </p>
      )}
      {error !== null && <Notice tone="danger">{error}</Notice>}
      {backups !== null && backups.length > 0 && (
        <ul className={styles.rooms} aria-label={t.list}>
          {backups.map((backup) => (
            <li key={backup.id} className={styles.room}>
              <span>{when(backup)}</span>
              <span className={styles.muted}>
                {b.reason[backup.reason]} ·{' '}
                {b.size({
                  files: backup.fileCount,
                  total: formatBytes(backup.totalBytes),
                  added: formatBytes(backup.newBytes),
                })}
              </span>
              <div className={styles.actions}>
                <Button
                  disabled={busy || waiting}
                  onClick={() => {
                    setRestoring(backup);
                  }}
                >
                  {b.restore}
                </Button>
              </div>
            </li>
          ))}
        </ul>
      )}
      {restoring !== null && (
        <Dialog
          title={world.name}
          onClose={() => {
            setRestoring(null);
          }}
        >
          <p>{t.confirmRestore({ date: when(restoring), world: world.name })}</p>
          <div className={forms.actions}>
            <Button
              onClick={() => {
                setRestoring(null);
              }}
            >
              {strings.worlds.cancel}
            </Button>
            <Button
              variant="primary"
              onClick={() => {
                const chosen = restoring;
                setRestoring(null);
                run(async () => {
                  await ipc.worldsRestore(world.id, chosen.id);
                  return t.restored({ date: when(chosen) });
                });
              }}
            >
              {b.restore}
            </Button>
          </div>
        </Dialog>
      )}
    </section>
  );
}
