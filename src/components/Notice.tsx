import type { ReactNode } from 'react';
import { Icon } from './Icon';
import styles from './Notice.module.css';

interface NoticeProps {
  tone: 'info' | 'warning' | 'danger';
  /** Errors interrupt screen readers; other notices are announced politely. */
  children: ReactNode;
}

const icons = { info: 'info', warning: 'warn', danger: 'warn' } as const;

/** A line on a tinted ground with its tone's icon at the inline start (concept A). */
export function Notice({ tone, children }: NoticeProps) {
  const classes = [styles.notice, styles[tone]].filter(Boolean).join(' ');
  return (
    <div className={classes} role={tone === 'danger' ? 'alert' : 'status'}>
      <Icon name={icons[tone]} size={18} className={styles.icon} />
      <div className={styles.text}>{children}</div>
    </div>
  );
}
