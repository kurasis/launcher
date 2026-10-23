import { useRef } from 'react';
import { Button } from '../../components/Button';
import { Dialog } from '../../components/Dialog';
import dialog from '../../components/Dialog.module.css';
import { Notice } from '../../components/Notice';
import { ProgressBar } from '../../components/ProgressBar';
import type { UpdateView } from '../../lib/contracts';
import { ipc } from '../../lib/ipc';
import { strings } from '../../lib/strings';
import { useCommand } from '../../lib/useCommand';
import { notesFor } from './notes';
import styles from './Updates.module.css';

const t = strings.updates;

/** The update's version, notes and what can be done with it now (D-063). */
export function UpdateDialog({ update, onClose }: { update: UpdateView; onClose: () => void }) {
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

  return (
    // Close (or Later) takes the focus: Enter never restarts the launcher by accident.
    <Dialog
      title={update.version === null ? t.titleUnknown : t.title({ version: update.version })}
      initialFocus={closeRef}
      onClose={onClose}
    >
      {update.mandatory && <Notice tone="warning">{t.mandatory}</Notice>}
      {update.phase === 'available' && <p>{t.availableBody}</p>}
      {update.phase === 'downloading' && (
        <>
          <p role="status">
            {percent === null ? t.downloading : t.downloadingPercent({ percent })}
          </p>
          <ProgressBar
            value={update.total === null ? null : update.downloaded}
            max={update.total ?? undefined}
            label={t.downloading}
          />
        </>
      )}
      {update.phase === 'ready' && <p>{t.readyBody}</p>}
      {update.phase === 'notice' && <p>{t.noticeBody}</p>}
      {/* The notes come from the release: text only, in their own direction. */}
      <p className={[dialog.hint, styles.notes].filter(Boolean).join(' ')}>
        {notes === null ? (
          t.noNotes
        ) : (
          <>
            {t.notesLead} <bdi>{notes}</bdi>
          </>
        )}
      </p>
      {update.phase === 'failed' && update.error !== null && (
        <Notice tone="danger">{t.errors[update.error]}</Notice>
      )}
      {error !== null && <Notice tone="danger">{error}</Notice>}
      <div className={dialog.actions}>
        {update.phase === 'available' && (
          <Button
            variant="primary"
            size="small"
            disabled={download.pending}
            onClick={() => void download.run()}
          >
            {t.download}
          </Button>
        )}
        {update.phase === 'ready' && (
          <Button
            variant="primary"
            size="small"
            disabled={restart.pending}
            onClick={() => void restart.run()}
          >
            {t.restartNow}
          </Button>
        )}
        {(update.phase === 'notice' ||
          (update.phase === 'failed' && update.downloadUrl !== null)) && (
          <Button variant="primary" size="small" onClick={() => void open.run()}>
            {t.openDownload}
          </Button>
        )}
        {update.phase === 'failed' && (
          <Button size="small" disabled={check.pending} onClick={() => void check.run()}>
            {t.tryAgain}
          </Button>
        )}
        <Button ref={closeRef} size="small" onClick={onClose}>
          {update.phase === 'ready' ? t.later : t.close}
        </Button>
      </div>
    </Dialog>
  );
}
