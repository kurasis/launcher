import { useEffect, useId, useRef, useState } from 'react';
import { Button } from '../../components/Button';
import { Notice } from '../../components/Notice';
import type { Delivery, ReportPreview, ReportSubject } from '../../lib/contracts';
import { describeIpcError, ipc } from '../../lib/ipc';
import { strings } from '../../lib/strings';
import styles from './Reports.module.css';

const t = strings.reports;
/** As much of a file as the preview shows; the whole file is sent. */
const SHOWN_CHARS = 200_000;
const MAX_NOTE = 2000;

interface ReportDialogProps {
  subject: ReportSubject;
  onClose: () => void;
  /** The report was sent or queued. */
  onSent?: (delivery: Delivery) => void;
}

/**
 * What a crash report would send, scrubbed, with a choice of files and a note
 * (D-061). Nothing leaves the computer until Send is pressed.
 */
export function ReportDialog({ subject, onClose, onSent }: ReportDialogProps) {
  const titleId = useId();
  const noteId = useId();
  const [preview, setPreview] = useState<ReportPreview | null>(null);
  const [chosen, setChosen] = useState<Set<string>>(new Set());
  const [note, setNote] = useState('');
  const [error, setError] = useState<string | null>(null);
  const [sending, setSending] = useState(false);
  const [delivery, setDelivery] = useState<Delivery | null>(null);
  const cancelRef = useRef<HTMLButtonElement>(null);
  const subjectKey = JSON.stringify(subject);

  useEffect(() => {
    let current = true;
    cancelRef.current?.focus();
    ipc
      .previewCrashReport(JSON.parse(subjectKey) as ReportSubject)
      .then((found) => {
        if (!current) return;
        setPreview(found);
        setChosen(new Set(found.files.map((f) => f.name)));
      })
      .catch((reason: unknown) => {
        if (current) setError(describeIpcError(reason));
      });
    return () => {
      current = false;
    };
  }, [subjectKey]);

  const send = () => {
    if (preview === null) return;
    setSending(true);
    setError(null);
    const files = preview.files.map((f) => f.name).filter((name) => chosen.has(name));
    ipc
      .sendCrashReport(preview.id, files, note.trim())
      .then((result) => {
        setDelivery(result);
        onSent?.(result);
      })
      .catch((reason: unknown) => {
        setError(describeIpcError(reason));
      })
      .finally(() => {
        setSending(false);
      });
  };

  const toggle = (name: string, on: boolean) => {
    setChosen((old) => {
      const next = new Set(old);
      if (on) next.add(name);
      else next.delete(name);
      return next;
    });
  };

  return (
    <div className={styles.backdrop}>
      <div
        role="dialog"
        aria-modal="true"
        aria-labelledby={titleId}
        className={styles.dialog}
        onKeyDown={(event) => {
          if (event.key === 'Escape' && !sending) onClose();
        }}
      >
        <h2 id={titleId}>{t.title}</h2>
        {delivery !== null ? (
          <>
            <Notice tone="info">{delivery === 'sent' ? t.sent : t.queued}</Notice>
            <div className={styles.actions}>
              <Button variant="primary" onClick={onClose}>
                {t.close}
              </Button>
            </div>
          </>
        ) : (
          <>
            <p>{t.intro}</p>
            <p className={styles.muted}>{t.privacy}</p>
            {preview === null && error === null && <p role="status">{t.loading}</p>}
            {preview !== null && (
              <>
                <p className={styles.muted}>{t.removed(preview.removed)}</p>
                <fieldset className={styles.files} disabled={sending}>
                  <legend>{t.files}</legend>
                  {preview.files.map((file) => (
                    <div key={file.name} className={styles.file}>
                      <label className={styles.check}>
                        <input
                          type="checkbox"
                          checked={chosen.has(file.name)}
                          onChange={(event) => {
                            toggle(file.name, event.target.checked);
                          }}
                        />
                        <span dir="ltr">{file.name}</span>
                      </label>
                      <details>
                        <summary>{t.show}</summary>
                        <pre className={styles.text} dir="ltr">
                          {file.text.length > SHOWN_CHARS
                            ? `${file.text.slice(0, SHOWN_CHARS)}\n…`
                            : file.text}
                        </pre>
                      </details>
                    </div>
                  ))}
                </fieldset>
                <label htmlFor={noteId} className={styles.label}>
                  {t.note}
                </label>
                <textarea
                  id={noteId}
                  className={styles.note}
                  value={note}
                  maxLength={MAX_NOTE}
                  rows={3}
                  placeholder={t.noteHint}
                  disabled={sending}
                  onChange={(event) => {
                    setNote(event.target.value);
                  }}
                />
              </>
            )}
            {error !== null && <Notice tone="danger">{error}</Notice>}
            <div className={styles.actions}>
              <Button
                variant="primary"
                onClick={send}
                disabled={preview === null || chosen.size === 0 || sending}
              >
                {sending ? t.sending : t.send}
              </Button>
              <Button ref={cancelRef} onClick={onClose} disabled={sending}>
                {t.cancel}
              </Button>
            </div>
          </>
        )}
      </div>
    </div>
  );
}
