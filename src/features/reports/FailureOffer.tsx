import { useCallback, useEffect, useState } from 'react';
import { Button } from '../../components/Button';
import { Notice } from '../../components/Notice';
import type { FailureSummary } from '../../lib/contracts';
import { describeIpcError, ipc } from '../../lib/ipc';
import { strings } from '../../lib/strings';
import { openReview } from '../reviews/openReview';
import { ReportDialog } from './ReportDialog';
import styles from './Reports.module.css';

const t = strings.reports;
/** Older failures are not offered any more; the export still has them. */
const RECENT_MS = 14 * 24 * 60 * 60 * 1000;

/**
 * Offers to send the newest failure of the launcher itself (D-061): a panic,
 * an error in the window or a session that did not end normally.
 */
export function FailureOffer() {
  const [failure, setFailure] = useState<FailureSummary | null>(null);
  const [open, setOpen] = useState(false);
  const [error, setError] = useState<string | null>(null);

  const reload = useCallback(() => {
    ipc
      .listLauncherFailures()
      .then((all) => {
        const now = Date.now();
        setFailure(
          all.find((f) => !f.reported && now - new Date(f.at).getTime() < RECENT_MS) ?? null,
        );
      })
      .catch(() => {
        setFailure(null);
      });
  }, []);

  useEffect(reload, [reload]);

  if (failure === null) return null;

  return (
    <Notice tone="warning">
      <div className={styles.offer}>
        <p>
          <strong>{t.failureTitle}</strong>
        </p>
        <p>{t.failureKinds[failure.kind]}</p>
        <p>{t.failureBody}</p>
        {error !== null && <p>{error}</p>}
        <div className={styles.actions}>
          <Button
            variant="primary"
            onClick={() => {
              setOpen(true);
            }}
          >
            {t.review}
          </Button>
          <Button
            onClick={() => {
              openReview({ kind: 'bug', category: 'launcher', diagnosticId: failure.id });
            }}
          >
            {t.tellMore}
          </Button>
          <Button
            onClick={() => {
              ipc
                .dismissLauncherFailure(failure.id)
                .then(reload)
                .catch((reason: unknown) => {
                  setError(describeIpcError(reason));
                });
            }}
          >
            {t.dismiss}
          </Button>
        </div>
      </div>
      {open && (
        <ReportDialog
          subject={{ type: 'launcher', failureId: failure.id }}
          onClose={() => {
            setOpen(false);
            reload();
          }}
        />
      )}
    </Notice>
  );
}
