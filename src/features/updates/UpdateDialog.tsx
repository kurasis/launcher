import { useEffect, useId, useRef } from 'react';
import { Button } from '../../components/Button';
import { Notice } from '../../components/Notice';
import type { UpdateView } from '../../lib/contracts';
import { ipc } from '../../lib/ipc';
import { strings } from '../../lib/strings';
import { useCommand } from '../../lib/useCommand';
import { notesFor } from './notes';
import styles from './Updates.module.css';

const t = strings.updates;

/** The update's version, notes and what can be done with it now (D-063). */
export function UpdateDialog({ update, onClose }: { update: UpdateView; onClose: () => void }) {
  const titleId = useId();
  const closeRef = useRef<HTMLButtonElement>(null);
  const download = useCommand(() => ipc.downloadUpdate());
  const restart = useCommand(() => ipc.restartToUpdate());
  const check = useCommand(() => ipc.checkForUpdate());
  const open = useCommand(() => ipc.openUpdatePage());
  const notes = notesFor(update.notes);
  const percent =
    update.total !== null && update.total > 0
      ? Math.min(100, Math.floor((update.downloaded / update.total) * 100))
      : null;
  const error = download.error ?? restart.error ?? check.error ?? open.error;

  useEffect(() => {
    closeRef.current?.focus();
  }, []);

  return (
    <div className={styles.backdrop}>
      <div
        role="dialog"
        aria-modal="true"
        aria-labelledby={titleId}
        className={styles.dialog}
        onKeyDown={(event) => {
          if (event.key === 'Escape') onClose();
        }}
      >
        <h2 id={titleId}>
          {update.version === null ? t.titleUnknown : t.title({ version: update.version })}
        </h2>
        {update.mandatory && <Notice tone="warning">{t.mandatory}</Notice>}
        <p className={styles.notes} dir="auto">
          {notes ?? t.noNotes}
        </p>
        {update.phase === 'available' && <p>{t.availableBody}</p>}
        {update.phase === 'downloading' && (
          <>
            <p role="status">
              {percent === null ? t.downloading : t.downloadingPercent({ percent })}
            </p>
            <progress value={update.downloaded} max={update.total ?? undefined} />
          </>
        )}
        {update.phase === 'ready' && <p>{t.readyBody}</p>}
        {update.phase === 'notice' && <p>{t.noticeBody}</p>}
        {update.phase === 'failed' && update.error !== null && (
          <Notice tone="danger">{t.errors[update.error]}</Notice>
        )}
        {error !== null && <Notice tone="danger">{error}</Notice>}
        <div className={styles.actions}>
          {update.phase === 'available' && (
            <Button
              variant="primary"
              disabled={download.pending}
              onClick={() => void download.run()}
            >
              {t.download}
            </Button>
          )}
          {update.phase === 'ready' && (
            <Button variant="primary" disabled={restart.pending} onClick={() => void restart.run()}>
              {t.restartNow}
            </Button>
          )}
          {(update.phase === 'notice' ||
            (update.phase === 'failed' && update.downloadUrl !== null)) && (
            <Button variant="primary" onClick={() => void open.run()}>
              {t.openDownload}
            </Button>
          )}
          {update.phase === 'failed' && (
            <Button disabled={check.pending} onClick={() => void check.run()}>
              {t.tryAgain}
            </Button>
          )}
          <Button ref={closeRef} onClick={onClose}>
            {update.phase === 'ready' ? t.later : t.close}
          </Button>
        </div>
      </div>
    </div>
  );
}
