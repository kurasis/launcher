import { useId, type ReactElement, type ReactNode } from 'react';
import styles from './Field.module.css';

interface FieldProps {
  label: string;
  hint?: ReactNode;
  /** Renders the control with the generated id and description wiring. */
  children: (props: { id: string; 'aria-describedby'?: string }) => ReactElement;
}

/** Label, control and optional hint, linked for assistive technology. */
export function Field({ label, hint, children }: FieldProps) {
  const id = useId();
  const hintId = `${id}-hint`;
  const describedBy = hint === undefined ? {} : { 'aria-describedby': hintId };
  return (
    <div className={styles.field}>
      <label htmlFor={id} className={styles.label}>
        {label}
      </label>
      {children({ id, ...describedBy })}
      {hint !== undefined && (
        <p id={hintId} className={styles.hint}>
          {hint}
        </p>
      )}
    </div>
  );
}
