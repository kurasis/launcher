import { useState } from 'react';
import { Button } from '../../components/Button';
import type { CrashDiagnosis } from '../../lib/contracts';
import { describeIpcError, ipc } from '../../lib/ipc';
import { strings } from '../../lib/strings';
import styles from './Reports.module.css';

const t = strings.reports;

/**
 * "Did this help?" under a crash explanation (D-061 stage 3). Sends only the
 * crash kind, its signature and the answer, and only when a button is pressed.
 */
export function ExplanationFeedback({ diagnosis }: { diagnosis: CrashDiagnosis }) {
  const [state, setState] = useState<'ask' | 'sending' | 'done'>('ask');
  const [error, setError] = useState<string | null>(null);

  const answer = (helpful: boolean) => {
    setState('sending');
    setError(null);
    ipc
      .sendExplanationFeedback(diagnosis.kind, diagnosis.signature ?? '', helpful)
      .then(() => {
        setState('done');
      })
      .catch((reason: unknown) => {
        setState('ask');
        setError(describeIpcError(reason));
      });
  };

  if (state === 'done') {
    return (
      <p className={styles.muted} role="status">
        {t.thanks}
      </p>
    );
  }
  return (
    <div className={styles.feedback}>
      <span className={styles.muted}>{t.helped}</span>
      <Button
        size="small"
        disabled={state === 'sending'}
        onClick={() => {
          answer(true);
        }}
      >
        {t.yes}
      </Button>
      <Button
        size="small"
        disabled={state === 'sending'}
        onClick={() => {
          answer(false);
        }}
      >
        {t.no}
      </Button>
      {error !== null && <span className={styles.muted}>{error}</span>}
    </div>
  );
}
