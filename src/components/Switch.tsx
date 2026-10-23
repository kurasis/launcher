import { useId, type ReactNode } from 'react';
import styles from './Switch.module.css';

interface SwitchProps {
  /** What it turns on, shown beside it: its accessible name. */
  label: ReactNode;
  /** More about it, under it: its accessible description. */
  hint?: ReactNode;
  checked: boolean;
  /** It cannot be changed (the setting cannot be written). */
  disabled?: boolean;
  /** A change is being saved: presses wait, but the focus stays (a disabled control
   * loses it in WebView2 and WebKit). */
  pending?: boolean;
  className?: string | undefined;
  onChange: (on: boolean) => void;
}

/**
 * An on/off setting that applies at once (concept A): a native checkbox with the role
 * of a switch, drawn as a track with a knob that moves to the inline end when on.
 */
export function Switch({
  label,
  hint,
  checked,
  disabled = false,
  pending = false,
  className,
  onChange,
}: SwitchProps) {
  const hintId = useId();
  const control = (
    <label className={[styles.switch, className].filter(Boolean).join(' ')}>
      <input
        type="checkbox"
        role="switch"
        checked={checked}
        disabled={disabled}
        aria-disabled={pending || undefined}
        aria-describedby={hint === undefined ? undefined : hintId}
        onChange={(event) => {
          if (!pending) onChange(event.target.checked);
        }}
      />
      <span>{label}</span>
    </label>
  );
  if (hint === undefined) return control;
  return (
    <div className={styles.field}>
      {control}
      <p id={hintId} className={styles.hint}>
        {hint}
      </p>
    </div>
  );
}
