import { useId, type ReactNode } from 'react';
import styles from './Panel.module.css';

interface PanelProps {
  title: string;
  /** Optional element rendered next to the heading, e.g. a status badge. */
  aside?: ReactNode;
  children: ReactNode;
}

/** A titled content section exposed to assistive technology as a labelled region. */
export function Panel({ title, aside, children }: PanelProps) {
  const headingId = useId();
  return (
    <section className={styles.panel} aria-labelledby={headingId}>
      <header className={styles.header}>
        <h2 id={headingId} className={styles.title}>
          {title}
        </h2>
        {aside}
      </header>
      <div className={styles.body}>{children}</div>
    </section>
  );
}
