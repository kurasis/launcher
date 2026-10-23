import styles from './ProgressBar.module.css';

interface ProgressBarProps {
  /** How far, out of max; null while it is not known (the bar shows only its track). */
  value: number | null;
  max?: number | undefined;
  /** The bar's name for assistive technology. */
  label: string;
  className?: string | undefined;
}

/** Concept A's progress bar: a thin accent line on the border-colored track. */
export function ProgressBar({ value, max = 100, label, className }: ProgressBarProps) {
  const shown = value === null || !Number.isFinite(value) ? undefined : Math.max(0, value);
  return (
    <progress
      className={[styles.bar, className].filter(Boolean).join(' ')}
      value={shown}
      max={max > 0 ? max : 100}
      aria-label={label}
    />
  );
}
