import { useEffect, useId, useRef, useState } from 'react';
import { createPortal } from 'react-dom';
import { Button } from '../../components/Button';
import { coveredByDialog } from '../../components/dialogStack';
import { Notice } from '../../components/Notice';
import { StatusBadge } from '../../components/StatusBadge';
import type { AppSnapshot, FeedProblem, NoteCard, NoteView } from '../../lib/contracts';
import { feedProblem, ipc } from '../../lib/ipc';
import { strings } from '../../lib/strings';
import { NoteBlocks } from './NoteBlocks';
import { kindLabel } from './parts';
import { pictureUrl } from './pictureUrl';
import { VersionAction } from './VersionAction';
import { shortDate } from './when';
import styles from './Feed.module.css';

const t = strings.feed;

interface NotePanelProps {
  /** The card the note was opened from: shown while the note loads. */
  card: NoteCard;
  snapshot: AppSnapshot;
  onClose: () => void;
  /** Opens a link of the note in the browser. */
  onLink: (url: string) => void;
  /** Opens Installations with this installation selected. */
  onOpenInstallations: (instanceId: string | null) => void;
  /** Opens the new-installation form with this game version chosen. */
  onInstallVersion: (versionId: string) => void;
}

/**
 * A whole patch note or news article in a dialog in the middle of the
 * window. A machine-translated note can show its English original; a
 * version's note has Play or Install for that version.
 */
export function NotePanel({
  card,
  snapshot,
  onClose,
  onLink,
  onOpenInstallations,
  onInstallVersion,
}: NotePanelProps) {
  const titleId = useId();
  const closeRef = useRef<HTMLButtonElement>(null);
  const [original, setOriginal] = useState(false);
  const [loaded, setLoaded] = useState<{ original: boolean; view: NoteView } | null>(null);
  const [failed, setFailed] = useState<{ original: boolean; problem: FeedProblem } | null>(null);
  const view = loaded?.original === original ? loaded.view : null;
  const problem = failed?.original === original ? failed.problem : null;
  const note = view?.note ?? null;
  const shown = note ?? card;
  const image = pictureUrl(shown.image);

  useEffect(() => {
    let current = true;
    ipc
      .feedNote(card.id, original)
      .then((answer) => {
        if (current) setLoaded({ original, view: answer });
      })
      .catch((error: unknown) => {
        if (current) setFailed({ original, problem: feedProblem(error) });
      });
    return () => {
      current = false;
    };
  }, [card.id, original]);

  // Focus moves into the dialog and back to what opened it.
  useEffect(() => {
    const opener = document.activeElement;
    closeRef.current?.focus();
    return () => {
      if (opener instanceof HTMLElement) opener.focus();
    };
  }, []);

  useEffect(() => {
    const listener = (event: KeyboardEvent) => {
      // Escape closes a dialog over the note (a download question) first.
      if (event.key === 'Escape' && !coveredByDialog(null)) onClose();
    };
    window.addEventListener('keydown', listener);
    return () => {
      window.removeEventListener('keydown', listener);
    };
  }, [onClose]);

  // A translated note, or its original now shown instead.
  const translatable = card.translated || original;

  return createPortal(
    <div
      className={styles.backdrop}
      onClick={(event) => {
        if (event.target === event.currentTarget) onClose();
      }}
    >
      <div role="dialog" aria-modal="true" aria-labelledby={titleId} className={styles.dialog}>
        <header className={styles.drawerHead}>
          <div className={styles.drawerTitle}>
            <div className={styles.badges}>
              <StatusBadge tone="neutral" label={kindLabel(shown)} />
              {shown.version !== null && (
                <span className={styles.muted} dir="ltr">
                  {shown.version}
                </span>
              )}
              <span className={styles.muted}>{shortDate(shown.date)}</span>
            </div>
            <div className={styles.titleRow}>
              <h2 id={titleId}>{shown.title}</h2>
              {shown.version !== null && (
                <div className={styles.titleAction}>
                  <VersionAction
                    snapshot={snapshot}
                    version={shown.version}
                    variant="primary"
                    onOpenInstallations={onOpenInstallations}
                    onInstallVersion={onInstallVersion}
                  />
                </div>
              )}
            </div>
          </div>
          <Button ref={closeRef} onClick={onClose}>
            {t.close}
          </Button>
        </header>
        {translatable && (
          <div className={styles.translation}>
            <button
              type="button"
              className={styles.pill}
              aria-pressed={original}
              onClick={() => {
                setOriginal((value) => !value);
              }}
            >
              {t.original}
            </button>
          </div>
        )}
        {view?.stale === true && <Notice tone="warning">{t.noteStale}</Notice>}
        <div className={styles.noteBody} lang={note?.lang ?? undefined}>
          {image !== null && <img className={styles.notePicture} src={image} alt="" />}
          {problem !== null && view === null ? (
            <Notice tone="danger">{t.problems[problem]}</Notice>
          ) : note === null ? (
            <p role="status">{t.noteLoading}</p>
          ) : (
            <NoteBlocks blocks={note.blocks} onLink={onLink} />
          )}
        </div>
        {shown.url !== null && (
          <footer className={styles.drawerActions}>
            <Button
              onClick={() => {
                if (shown.url !== null) onLink(shown.url);
              }}
            >
              {t.readOnSite}
            </Button>
          </footer>
        )}
      </div>
    </div>,
    document.body,
  );
}
