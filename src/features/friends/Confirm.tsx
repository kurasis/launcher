import { useEffect, useId, useRef } from 'react';
import { Button } from '../../components/Button';
import { strings } from '../../lib/strings';
import styles from './Friends.module.css';

/**
 * "Block Notch_1? They will not be told." in place, under what it is about: the focus
 * moves to it, Escape or Cancel closes it (and only it), and the answer is pressed.
 */
export function Confirm({
  label,
  text,
  action,
  pending,
  onYes,
  onNo,
}: {
  /** The name of the question for screen readers, such as the friend's nick. */
  label: string;
  text: string;
  /** The button that does it, such as "Block". */
  action: string;
  pending: boolean;
  onYes: () => void;
  onNo: () => void;
}) {
  const box = useRef<HTMLDivElement>(null);
  const textId = useId();
  useEffect(() => {
    box.current?.focus();
  }, []);
  return (
    <div
      ref={box}
      className={styles.confirm}
      role="alertdialog"
      aria-label={label}
      aria-describedby={textId}
      tabIndex={-1}
      onKeyDown={(event) => {
        if (event.key === 'Escape') {
          // Closes the question, not the whole sheet.
          event.stopPropagation();
          onNo();
        }
      }}
    >
      <p id={textId}>{text}</p>
      <div className={styles.inline}>
        <Button
          variant="danger"
          size="small"
          aria-disabled={pending || undefined}
          onClick={() => {
            if (!pending) onYes();
          }}
        >
          {action}
        </Button>
        <Button size="small" onClick={onNo}>
          {strings.friends.cancel}
        </Button>
      </div>
    </div>
  );
}
