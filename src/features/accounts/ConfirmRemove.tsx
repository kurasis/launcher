import { useEffect, useRef, useState } from 'react';
import { Button } from '../../components/Button';
import { strings } from '../../lib/strings';
import styles from './Accounts.module.css';

interface ConfirmRemoveProps {
  /** The question asked before removing. */
  question: string;
  disabled?: boolean;
  onRemove: () => void;
}

const a = strings.accounts;

/** A Remove button that asks first: a removed account needs signing in again. */
export function ConfirmRemove({ question, disabled = false, onRemove }: ConfirmRemoveProps) {
  const [asking, setAsking] = useState(false);
  const cancel = useRef<HTMLButtonElement>(null);
  const remove = useRef<HTMLButtonElement>(null);
  const backToRemove = useRef(false);
  // The pressed Remove is gone: focus waits on the safe answer, once, when the
  // question opens (never again on a later render), and comes back to Remove when
  // the question is cancelled.
  useEffect(() => {
    if (asking) {
      cancel.current?.focus();
    } else if (backToRemove.current) {
      backToRemove.current = false;
      remove.current?.focus();
    }
  }, [asking]);
  if (!asking) {
    return (
      <Button
        ref={remove}
        size="small"
        disabled={disabled}
        onClick={() => {
          setAsking(true);
        }}
      >
        {a.remove}
      </Button>
    );
  }
  return (
    <div className={styles.confirm} role="alertdialog" aria-label={question}>
      <span>{question}</span>
      <Button
        variant="primary"
        size="small"
        disabled={disabled}
        onClick={() => {
          setAsking(false);
          onRemove();
        }}
      >
        {a.remove}
      </Button>
      <Button
        ref={cancel}
        size="small"
        onClick={() => {
          backToRemove.current = true;
          setAsking(false);
        }}
      >
        {a.launcherCancel}
      </Button>
    </div>
  );
}
