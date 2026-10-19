import { useState } from 'react';
import { Button } from '../../components/Button';
import forms from '../../components/forms.module.css';
import { strings } from '../../lib/strings';

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
    <div className={forms.actions} role="alertdialog" aria-label={question}>
      <span>{question}</span>
      <Button
        variant="primary"
        disabled={disabled}
        onClick={() => {
          setAsking(false);
          onRemove();
        }}
      >
        {a.remove}
      </Button>
      <Button
        onClick={() => {
          setAsking(false);
        }}
      >
        {a.launcherCancel}
      </Button>
    </div>
  );
}
