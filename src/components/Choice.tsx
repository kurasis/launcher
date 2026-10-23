import { useId, type InputHTMLAttributes, type ReactNode } from 'react';
import styles from './Choice.module.css';

type ChoiceProps = Omit<InputHTMLAttributes<HTMLInputElement>, 'type' | 'title'> & {
  type: 'radio' | 'checkbox';
  /** The choice's name, beside the control. */
  title: ReactNode;
  /** What it means, under the title: the control's description. */
  hint?: ReactNode;
};

/**
 * A radio or checkbox with a title and a muted hint under it (concept A): no card
 * around it, the control in the accent color.
 */
export function Choice({ type, title, hint, ...input }: ChoiceProps) {
  const hintId = useId();
  return (
    <div className={styles.choice}>
      <label className={styles.label}>
        <input type={type} aria-describedby={hint === undefined ? undefined : hintId} {...input} />
        <span>{title}</span>
      </label>
      {hint !== undefined && (
        <span id={hintId} className={styles.hint}>
          {hint}
        </span>
      )}
    </div>
  );
}
