import { useEffect, useId, useRef, type ReactNode } from 'react';
import { Button } from './Button';
import styles from './Confirm.module.css';

/**
 * "Block Notch_1? They will not be told." in place, under what it is about: the focus
 * moves to it, Escape or Cancel closes it (and only it), and the answer is pressed.
 * While the answer goes, neither closes it: what was pressed is under way. While
 * something else runs (`held`), the answer waits but the question still closes. The
 * answer that deletes or stops is drawn as danger; one that only does something big
 * (copies the data) as primary.
 */
export function Confirm({
  label,
  text,
  children,
  action,
  actionVariant = 'danger',
  cancel,
  pending,
  held = false,
  onYes,
  onNo,
}: {
  /** The name of the question for screen readers, such as the friend's nick. */
  label: string;
  /** The question; or `children` for a longer one (a list of what goes). */
  text?: string;
  children?: ReactNode;
  /** The button that does it, such as "Block". */
  action: string;
  actionVariant?: 'danger' | 'primary';
  /** The button that closes it. */
  cancel: string;
  pending: boolean;
  /** Something else runs: the answer cannot go yet, but Cancel and Escape still close. */
  held?: boolean;
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
          // Closes the question, not what it sits in (a sheet).
          event.preventDefault();
          event.stopPropagation();
          if (!pending) onNo();
        }
      }}
    >
      {text === undefined ? (
        <div id={textId} className={styles.text}>
          {children}
        </div>
      ) : (
        <p id={textId}>{text}</p>
      )}
      <div className={styles.actions}>
        <Button
          variant={actionVariant}
          size="small"
          aria-disabled={pending || held || undefined}
          onClick={() => {
            if (!pending && !held) onYes();
          }}
        >
          {action}
        </Button>
        <Button
          size="small"
          aria-disabled={pending || undefined}
          onClick={() => {
            if (!pending) onNo();
          }}
        >
          {cancel}
        </Button>
      </div>
    </div>
  );
}
