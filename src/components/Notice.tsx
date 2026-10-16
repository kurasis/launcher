import type { ReactNode } from 'react';
import styles from './Notice.module.css';

interface NoticeProps {
  tone: 'info' | 'warning' | 'danger';
  /** Errors interrupt screen readers; other notices are announced politely. */
  children: ReactNode;
}

export function Notice({ tone, children }: NoticeProps) {
  const classes = [styles.notice, styles[tone]].filter(Boolean).join(' ');
  return (
    <div className={classes} role={tone === 'danger' ? 'alert' : 'status'}>
      {children}
    </div>
  );
}
