import { useId, type ReactNode } from 'react';
import styles from './Panel.module.css';

interface PanelProps {
  title: string;
  /** The heading's level: 2 for a screen's own panels, deeper inside another section. */
  level?: 2 | 3 | 4;
  /** Optional element rendered next to the heading, e.g. a status badge. */
  aside?: ReactNode;
  /** The aside follows the heading instead of sitting at the far end. */
  asideAtTitle?: boolean;
  children: ReactNode;
}

/** A titled content section exposed to assistive technology as a labelled region. */
export function Panel({ title, level = 2, aside, asideAtTitle = false, children }: PanelProps) {
  const headingId = useId();
  const Heading = `h${String(level)}` as 'h2' | 'h3' | 'h4';
  return (
    <section className={styles.panel} aria-labelledby={headingId}>
      <header className={[styles.header, asideAtTitle && styles.atTitle].filter(Boolean).join(' ')}>
        <Heading id={headingId} className={styles.title}>
          {title}
        </Heading>
        {aside}
      </header>
      <div className={styles.body}>{children}</div>
    </section>
  );
}
