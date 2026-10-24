import type { ReactNode } from 'react';
import styles from './LiveStatus.module.css';

/**
 * A polite live region that stays mounted whatever it says, so each new text is read
 * out; while it says nothing it takes no room. Empty it while a new answer is awaited,
 * so the old one is not read again.
 */
export function LiveStatus({
  children,
  className,
}: {
  children: ReactNode;
  className?: string | undefined;
}) {
  return (
    <p role="status" className={[styles.status, className].filter(Boolean).join(' ')}>
      {children}
    </p>
  );
}
