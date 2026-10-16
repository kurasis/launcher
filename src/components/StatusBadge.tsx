import styles from './StatusBadge.module.css';

export type StatusTone = 'neutral' | 'success' | 'warning' | 'danger';

interface StatusBadgeProps {
  tone: StatusTone;
  label: string;
}

/** Short status text. The tone is decorative; the label must carry the full meaning. */
export function StatusBadge({ tone, label }: StatusBadgeProps) {
  const classes = [styles.badge, styles[tone]].filter(Boolean).join(' ');
  return <span className={classes}>{label}</span>;
}
