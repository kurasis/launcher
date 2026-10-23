import { useState } from 'react';
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
  if (!asking) {
    return (
      <Button
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
      {/* The pressed Remove is gone: focus waits on the safe answer. */}
      <Button
        ref={(node) => {
          node?.focus();
        }}
        size="small"
        onClick={() => {
          setAsking(false);
        }}
      >
        {a.launcherCancel}
      </Button>
    </div>
  );
}
