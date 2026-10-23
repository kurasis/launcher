import styles from './StatusBadge.module.css';

export type StatusTone = 'neutral' | 'success' | 'warning' | 'danger';

interface StatusBadgeProps {
  tone: StatusTone;
  label: string;
  className?: string | undefined;
  /** False for a chip that names a kind (a crash's), not a state: no dot, and it wraps. */
  dot?: boolean;
}

/** Short status text. The tone is decorative; the label must carry the full meaning. */
export function StatusBadge({ tone, label, className, dot = true }: StatusBadgeProps) {
  const classes = [styles.badge, styles[tone], !dot && styles.chip, className]
    .filter(Boolean)
    .join(' ');
  return (
    <span className={classes} title={className !== undefined ? label : undefined}>
      {label}
    </span>
  );
}
