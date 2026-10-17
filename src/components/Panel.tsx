import { useId, type ReactNode } from 'react';
import { HeroArt } from './HeroArt';
import styles from './Panel.module.css';

interface PanelProps {
  title: string;
  /** Optional element rendered next to the heading, e.g. a status badge. */
  aside?: ReactNode;
  /** "hero" draws a large heading over a decorative landscape. */
  variant?: 'default' | 'hero';
  children: ReactNode;
}

/** A titled content section exposed to assistive technology as a labelled region. */
export function Panel({ title, aside, variant = 'default', children }: PanelProps) {
  const headingId = useId();
  const hero = variant === 'hero';
  return (
    <section
      className={[styles.panel, hero && styles.hero].filter(Boolean).join(' ')}
      aria-labelledby={headingId}
    >
      {hero && <HeroArt className={styles.art} />}
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
