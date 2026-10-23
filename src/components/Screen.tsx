import { useId, type ReactNode } from 'react';
import styles from './Screen.module.css';

interface ScreenProps {
  /** The screen's title: a level 2 heading, under the brand's. */
  title: string;
  /** Actions at the end of the head, such as "New installation". */
  actions?: ReactNode;
  className?: string | undefined;
  children: ReactNode;
}

/**
 * A screen of the main window in concept A (D-072): the title with its actions, then
 * the content. The screen is a size container named `screen`, so its layout follows the
 * room it has, with or without the rail's labels.
 */
export function Screen({ title, actions, className, children }: ScreenProps) {
  const headingId = useId();
  return (
    <section
      className={[styles.screen, className].filter(Boolean).join(' ')}
      aria-labelledby={headingId}
    >
      <header className={styles.head}>
        <h2 id={headingId} className={styles.title}>
          {title}
        </h2>
        {actions !== undefined && <div className={styles.actions}>{actions}</div>}
      </header>
      {children}
    </section>
  );
}
