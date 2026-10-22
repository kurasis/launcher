import styles from './StatusBadge.module.css';

export type StatusTone = 'neutral' | 'success' | 'warning' | 'danger';

interface StatusBadgeProps {
  tone: StatusTone;
  label: string;
  className?: string | undefined;
}

/** Short status text. The tone is decorative; the label must carry the full meaning. */
export function StatusBadge({ tone, label, className }: StatusBadgeProps) {
  const classes = [styles.badge, styles[tone], className].filter(Boolean).join(' ');
  return (
    <span className={classes} title={className !== undefined ? label : undefined}>
      {label}
    </span>
  );
}
