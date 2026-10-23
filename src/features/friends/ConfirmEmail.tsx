import { useState } from 'react';
import { Button } from '../../components/Button';
import { accountProblem, ipc } from '../../lib/ipc';
import { strings } from '../../lib/strings';
import styles from './Friends.module.css';
import { Spinner } from './Spinner';

const t = strings.friends;

/** The account has no confirmed email yet: what to do, right here. */
export function ConfirmEmail({ onOpenAccounts }: { onOpenAccounts: () => void }) {
  const a = strings.accounts;
  const [checking, setChecking] = useState(false);
  const [note, setNote] = useState<string | null>(null);
  const check = async () => {
    setChecking(true);
    setNote(null);
    try {
      // Confirmed: friends tries again by itself and this panel goes.
      const confirmed = await ipc.launcherAccountRecheck();
      if (!confirmed) setNote(a.launcherStillNotConfirmed);
    } catch (e) {
      setNote(a.launcherProblems[accountProblem(e)]);
    } finally {
      setChecking(false);
    }
  };
  return (
    <div className={styles.state}>
      <p>{t.emailNeeded}</p>
      <div className={styles.inline}>
        <Button
          variant="primary"
          aria-disabled={checking || undefined}
          onClick={() => {
            if (!checking) void check();
          }}
        >
          {checking ? a.launcherChecking : a.launcherCheck}
        </Button>
        <Button
          onClick={() => {
            setNote(null);
            void ipc
              .launcherAccountResendConfirmation()
              .then((problem) => {
                setNote(problem === null ? t.emailResent : a.launcherProblems[problem]);
              })
              .catch(() => {
                setNote(a.launcherProblems.server);
              });
          }}
        >
          {a.launcherResend}
        </Button>
      </div>
      {checking && <Spinner label={t.checking} />}
      {note !== null && (
        <p className={styles.muted} role="status">
          {note}
        </p>
      )}
      <button type="button" className={styles.link} onClick={onOpenAccounts}>
        {t.goToAccounts}
      </button>
    </div>
  );
}
