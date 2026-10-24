import type { ReactNode } from 'react';
import { Icon } from './Icon';
import styles from './Notice.module.css';

interface NoticeProps {
  tone: 'info' | 'warning' | 'danger';
  /** False keeps a notice whose text keeps changing (a growing count) out of the live
   * regions, so it is not read out on every change. */
  live?: boolean;
  /** Errors interrupt screen readers; other notices are announced politely. */
  children: ReactNode;
}

const icons = { info: 'info', warning: 'warn', danger: 'warn' } as const;

/** A line on a tinted ground with its tone's icon at the inline start (concept A). */
export function Notice({ tone, live = true, children }: NoticeProps) {
  const classes = [styles.notice, styles[tone]].filter(Boolean).join(' ');
  const role = !live ? undefined : tone === 'danger' ? 'alert' : 'status';
  return (
    <div className={classes} role={role}>
      <Icon name={icons[tone]} size={18} className={styles.icon} />
      <div className={styles.text}>{children}</div>
    </div>
  );
}
