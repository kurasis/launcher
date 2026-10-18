import { useEffect, useRef, useState } from 'react';
import { Button } from '../components/Button';
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
}

export function QuitControl({ activeKind = null }: QuitControlProps) {
  const [choosing, setChoosing] = useState(false);
  const [error, setError] = useState<string | null>(null);
  const keepRef = useRef<HTMLButtonElement>(null);
  const job = activeKind === 'install' || activeKind === 'repair';

  useEffect(() => {
    if (choosing) keepRef.current?.focus();
  }, [choosing]);

  useEffect(
    () =>
      onQuitRequested(() => {
        setChoosing(true);
      }),
    [],
  );

  const send = async (action: QuitAction) => {
    setError(null);
    try {
      const result = await ipc.requestQuit(action);
      setChoosing(result.status === 'choice_required');
    } catch (e) {
      setError(describeIpcError(e));
    }
  };

  return (
    <>
      <Button
        onClick={() => {
          void send('quit');
        }}
      >
        {t.button}
      </Button>
      {!choosing && error !== null && (
        <p role="alert" className={styles.error}>
          {error}
        </p>
      )}
      {choosing && (
        <div className={styles.backdrop}>
          <div
            role="dialog"
            aria-modal="true"
            aria-labelledby="quit-title"
            aria-describedby="quit-body"
            className={styles.dialog}
            onKeyDown={(event) => {
              if (event.key === 'Escape') setChoosing(false);
            }}
          >
            <h2 id="quit-title">{job ? t.jobTitle : t.dialogTitle}</h2>
            <p id="quit-body">{job ? t.jobBody : t.dialogBody}</p>
            {error !== null && <Notice tone="danger">{error}</Notice>}
            <div className={styles.actions}>
              <Button
                ref={keepRef}
                variant="primary"
                onClick={() => {
                  void send('keep_in_background');
                }}
              >
                {t.keep}
              </Button>
              <Button
                onClick={() => {
                  void send('stop_and_quit');
                }}
              >
                {job ? t.jobStop : t.stop}
              </Button>
              <Button
                onClick={() => {
                  setChoosing(false);
                }}
              >
                {t.cancel}
              </Button>
            </div>
          </div>
        </div>
      )}
    </>
  );
}
