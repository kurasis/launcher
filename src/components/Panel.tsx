import { useId, type ReactNode } from 'react';
import { numberLocale } from '../lib/strings';
import styles from './Panel.module.css';

interface PanelProps {
  /** The panel's heading. Without one the panel is only a surface around its content,
   * such as the first panel of a settings section, which the section names already. */
  title?: string;
  /** The heading's level: 2 for a screen's own panels, deeper inside another section. */
  level?: 2 | 3 | 4;
  /** The heading's id, for a group inside that its title names too. */
  headingId?: string;
  /** How many things the panel lists, after its title ("Rooms 4"). */
  count?: number | undefined;
  /** Optional element rendered next to the heading, e.g. a status badge. */
  aside?: ReactNode;
  /** The aside follows the heading instead of sitting at the far end. */
  asideAtTitle?: boolean;
  /** One action for the whole panel, at its end beside the heading and the text (Open
   * console); it goes under them when there is no room. */
  action?: ReactNode;
  children: ReactNode;
}

/** A titled content section exposed to assistive technology as a labelled region. */
export function Panel({
  title,
  level = 2,
  headingId,
  count,
  aside,
  asideAtTitle = false,
  action,
  children,
}: PanelProps) {
  const ownId = useId();
  const id = headingId ?? ownId;
  if (title === undefined) {
    return (
      <div className={styles.panel}>
        <div className={styles.body}>{children}</div>
      </div>
    );
  }
  const Heading = `h${String(level)}` as 'h2' | 'h3' | 'h4';
  const content = (
    <>
      <header className={[styles.header, asideAtTitle && styles.atTitle].filter(Boolean).join(' ')}>
        <Heading id={id} className={styles.title}>
          {title}
          {/* The space keeps "Rooms 4" apart for assistive technology too. */}
          {count !== undefined && ' '}
          {count !== undefined && (
            <span className={styles.count}>{count.toLocaleString(numberLocale())}</span>
          )}
        </Heading>
        {aside}
      </header>
      <div className={styles.body}>{children}</div>
    </>
  );
  if (action !== undefined) {
    return (
      <section className={[styles.panel, styles.withAction].join(' ')} aria-labelledby={id}>
        <div className={styles.main}>{content}</div>
        <div className={styles.action}>{action}</div>
      </section>
    );
  }
  return (
    <section className={styles.panel} aria-labelledby={id}>
      {content}
    </section>
  );
}
