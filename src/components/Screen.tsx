import { useId, type ReactNode, type Ref } from 'react';
import styles from './Screen.module.css';

interface ScreenProps {
  /** The screen's title: a level 2 heading, under the brand's. */
  title: string;
  /** The title is a name someone gave (a room's): it keeps its own direction. */
  isolateTitle?: boolean;
  /** A way back before the title, on a screen opened from another ("‹ Wardrobe"). */
  back?: ReactNode;
  /** A way back on its own line above the title, for a long title with facts after it. */
  top?: ReactNode;
  /** Facts right after the title, such as tags. */
  aside?: ReactNode;
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
export function Screen({
  title,
  isolateTitle = false,
  back,
  top,
  aside,
  actions,
  headingRef,
  className,
  children,
}: ScreenProps) {
  const headingId = useId();
  const heading = (
    <h2
      id={headingId}
      ref={headingRef}
      className={styles.title}
      tabIndex={headingRef === undefined ? undefined : -1}
    >
      {isolateTitle ? <bdi>{title}</bdi> : title}
    </h2>
  );
  return (
    <section
      className={[styles.screen, className].filter(Boolean).join(' ')}
      aria-labelledby={headingId}
    >
      {top !== undefined && <div className={styles.top}>{top}</div>}
      <header className={styles.head}>
        {/* The title keeps its place while the facts after it come and go, so a focused
            title stays focused. */}
        {back === undefined && top === undefined && aside === undefined ? (
          heading
        ) : (
          <div className={styles.titleRow}>
            {back}
            {heading}
            {aside !== undefined && <div className={styles.aside}>{aside}</div>}
          </div>
        )}
        {actions !== undefined && <div className={styles.actions}>{actions}</div>}
      </header>
      {children}
    </section>
  );
}
