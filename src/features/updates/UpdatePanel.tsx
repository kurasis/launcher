import { useRef } from 'react';
import { Button } from '../../components/Button';
import { Notice } from '../../components/Notice';
import { Panel } from '../../components/Panel';
import { ProgressBar } from '../../components/ProgressBar';
import { useFocusBack } from '../../components/useFocusBack';
import type { UpdateView } from '../../lib/contracts';
import { ipc } from '../../lib/ipc';
import { strings } from '../../lib/strings';
import { useCommand } from '../../lib/useCommand';
import { noteLines, notesFor } from './notes';
import styles from './Updates.module.css';

const t = strings.updates;

/** The notes of a version: a list when they are lines, as text only, scrolling when long. */
function Notes({ notes }: { notes: Record<string, string> }) {
  const text = notesFor(notes);
  const lines = text === null ? [] : noteLines(text);
  if (lines.length === 0) return <p className={styles.muted}>{t.noNotes}</p>;
  return (
    <div className={styles.notesBox} tabIndex={0} role="region" aria-label={t.notesLabel}>
      {lines.length === 1 ? (
        <p dir="auto">{lines[0]}</p>
      ) : (
        <ul className={styles.noteList}>
          {lines.map((line, index) => (
            // Lines can repeat: their place tells them apart.
            <li key={index} dir="auto">
              {line}
            </li>
          ))}
        </ul>
      )}
    </div>
  );
}

/**
 * About → What's new (D-063): the version waiting to be installed, its notes and what can
 * be done with it now; or, once installed, the notes of the version now running until
 * they are read. Nothing when there is neither. One status line says where the update
 * stands; when the button that had the focus goes with its phase, the focus goes to that
 * line, never to Restart now, which closes the launcher.
 */
export function UpdatePanel({ update }: { update: UpdateView }) {
  const download = useCommand(() => ipc.downloadUpdate());
  const restart = useCommand(() => ipc.restartToUpdate());
  const check = useCommand(() => ipc.checkForUpdate());
  const open = useCommand(() => ipc.openUpdatePage());
  const error = download.error ?? restart.error ?? check.error ?? open.error;
  const line = useRef<HTMLParagraphElement>(null);
  useFocusBack(update.phase, line);

  if (update.phase === 'idle' || update.version === null) {
    if (update.whatsNew === null) return null;
    return (
      <Panel title={t.whatsNew({ version: update.whatsNew.version })} level={3}>
        <Notes notes={update.whatsNew.notes} />
      </Panel>
    );
  }
  const percent =
    update.total !== null && update.total > 0
      ? Math.min(100, Math.floor((update.downloaded / update.total) * 100))
      : null;
  // A press while its command goes does nothing; the button keeps the focus.
  const press = (command: { pending: boolean; run: () => Promise<unknown> }) => () => {
    if (!command.pending) void command.run();
  };

  const phaseLine = {
    available: t.availableBody,
    downloading: t.downloading,
    ready: t.readyShort,
    notice: t.noticeBody,
    failed: t.settings.phases.failed,
  }[update.phase];

  return (
    <Panel title={t.whatsNew({ version: update.version })} level={3}>
      {update.mandatory && <Notice tone="warning">{t.mandatory}</Notice>}
      <Notes notes={update.notes} />
      {update.phase === 'failed' && update.error !== null && (
        <Notice tone="danger">{t.errors[update.error]}</Notice>
      )}
      {error !== null && <Notice tone="danger">{error}</Notice>}
      <div className={styles.status}>
        {/* The same line in every phase, so that it is read out when it changes; the
            percent stays with the bar, not read out at each step. */}
        <p ref={line} role="status" className={styles.line} tabIndex={-1}>
          {phaseLine}
        </p>
        {update.phase === 'available' && (
          <Button
            size="small"
            aria-disabled={download.pending || undefined}
            onClick={press(download)}
          >
            {t.download}
          </Button>
        )}
        {update.phase === 'ready' && (
          <Button
            size="small"
            aria-disabled={restart.pending || undefined}
            onClick={press(restart)}
          >
            {t.restartNow}
          </Button>
        )}
        {(update.phase === 'notice' ||
          (update.phase === 'failed' && update.downloadUrl !== null)) && (
          <Button size="small" aria-disabled={open.pending || undefined} onClick={press(open)}>
            {t.openDownload}
          </Button>
        )}
        {update.phase === 'failed' && (
          <Button size="small" aria-disabled={check.pending || undefined} onClick={press(check)}>
            {t.tryAgain}
          </Button>
        )}
      </div>
      {update.phase === 'downloading' && (
        <div className={styles.progress}>
          <ProgressBar
            className={styles.wide}
            value={update.total === null ? null : update.downloaded}
            max={update.total ?? undefined}
            label={t.downloading}
          />
          {/* The bar tells assistive technology how far it is. */}
          {percent !== null && (
            <span className={styles.muted} aria-hidden="true">
              {strings.downloads.percent({ percent })}
            </span>
          )}
        </div>
      )}
    </Panel>
  );
}
