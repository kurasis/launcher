import { useEffect, useId, useRef, useState } from 'react';
import { Button } from '../components/Button';
import { Dialog } from '../components/Dialog';
import dialog from '../components/Dialog.module.css';
import { Icon } from '../components/Icon';
import { Notice } from '../components/Notice';
import type { OperationKind, QuitAction } from '../lib/contracts';
import { describeIpcError, ipc, onQuitRequested } from '../lib/ipc';
import { strings } from '../lib/strings';
import styles from './QuitControl.module.css';

const t = strings.quit;

/** Explicit Quit. With an active run the backend asks for a choice first. */
interface QuitControlProps {
  /** Kind of the active operation, which decides the wording of the choice. */
  activeKind?: OperationKind | null;
  /** An icon button, as in the rail; its name stays "Quit". */
  compact?: boolean;
}

export function QuitControl({ activeKind = null, compact = false }: QuitControlProps) {
  const [choosing, setChoosing] = useState(false);
  const [hosting, setHosting] = useState(false);
  const [error, setError] = useState<string | null>(null);
  const keepRef = useRef<HTMLButtonElement>(null);
  const bodyId = useId();
  const job = activeKind === 'install' || activeKind === 'repair';

  useEffect(
    () =>
      onQuitRequested((worldOpen) => {
        setChoosing(true);
        setHosting(worldOpen);
      }),
    [],
  );

  // Only a world open to friends, or a world beside a run or a job.
  const onlyHosting = hosting && activeKind === null;
  const title = onlyHosting ? t.hostingTitle : job ? t.jobTitle : t.dialogTitle;
  const body = onlyHosting
    ? t.hostingBody
    : `${job ? t.jobBody : t.dialogBody}${hosting ? ` ${t.hostingBody}` : ''}`;
  const stop = onlyHosting ? t.hostingStop : job ? t.jobStop : t.stop;

  const send = async (action: QuitAction) => {
    setError(null);
    try {
      const result = await ipc.requestQuit(action);
      setChoosing(result.status === 'choice_required');
      setHosting(result.status === 'choice_required' && result.hosting);
    } catch (e) {
      setError(describeIpcError(e));
    }
  };

  return (
    <>
      {compact ? (
        <button
          type="button"
          className={styles.iconButton}
          aria-label={t.button}
          title={t.button}
          onClick={() => {
            void send('quit');
          }}
        >
          <Icon name="power" size={18} />
        </button>
      ) : (
        <Button
          onClick={() => {
            void send('quit');
          }}
        >
          {t.button}
        </Button>
      )}
      {!choosing && error !== null && (
        <p role="alert" className={compact ? styles.railError : styles.error}>
          {error}
        </p>
      )}
      {choosing && (
        // On the page body, above the rail's popups: the system's close button asks too.
        <Dialog
          title={title}
          describedBy={bodyId}
          initialFocus={keepRef}
          onClose={() => {
            setChoosing(false);
          }}
        >
          <p id={bodyId}>{body}</p>
          {error !== null && <Notice tone="danger">{error}</Notice>}
          <div className={dialog.actions}>
            <Button
              ref={keepRef}
              variant="primary"
              size="small"
              onClick={() => {
                void send('keep_in_background');
              }}
            >
              {t.keep}
            </Button>
            <Button
              variant="danger"
              size="small"
              onClick={() => {
                void send('stop_and_quit');
              }}
            >
              {stop}
            </Button>
            <Button
              variant="ghost"
              size="small"
              onClick={() => {
                setChoosing(false);
              }}
            >
              {t.cancel}
            </Button>
          </div>
        </Dialog>
      )}
    </>
  );
}
