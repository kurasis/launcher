import { useId, type ReactNode, type Ref } from 'react';
import styles from './Screen.module.css';

interface ScreenProps {
  /** The screen's title: a level 2 heading, under the brand's. */
  title: string;
  /** A way back before the title, on a screen opened from another ("‹ Wardrobe"). */
  back?: ReactNode;
  /** Actions at the end of the head, such as "New installation". */
  actions?: ReactNode;
  /** The title, to take the focus when what it heads changes under it. */
  headingRef?: Ref<HTMLHeadingElement>;
  className?: string | undefined;
  children: ReactNode;
}

/**
 * A screen of the main window in concept A (D-072): the title with its actions, then
 * the content. The screen is a size container named `screen`, so its layout follows the
 * room it has, with or without the rail's labels.
 */
export function Screen({ title, back, actions, headingRef, className, children }: ScreenProps) {
  const headingId = useId();
  const heading = (
    <h2
      id={headingId}
      ref={headingRef}
      className={styles.title}
      tabIndex={headingRef === undefined ? undefined : -1}
    >
      {title}
    </h2>
  );
  return (
    <section
      className={[styles.screen, className].filter(Boolean).join(' ')}
      aria-labelledby={headingId}
    >
      <header className={styles.head}>
        {back === undefined ? (
          heading
        ) : (
          <div className={styles.titleRow}>
            {back}
            {heading}
          </div>
        )}
        {actions !== undefined && <div className={styles.actions}>{actions}</div>}
      </header>
      {children}
    </section>
  );
}
