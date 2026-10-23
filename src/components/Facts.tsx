import type { ReactNode } from 'react';
import styles from './Facts.module.css';

/** Facts as two columns, the name muted and the value beside it (concept A). */
export function Facts({ children }: { children: ReactNode }) {
  return <dl className={styles.facts}>{children}</dl>;
}

/** One fact of `Facts`. */
export function Fact({
  term,
  children,
  dir,
}: {
  term: ReactNode;
  children: ReactNode;
  /** The value's direction, such as `ltr` for a path or a version. It sits where the
   * page's direction puts values; only its own text runs that way. */
  dir?: 'ltr' | 'auto';
}) {
  return (
    <div className={styles.fact}>
      <dt>{term}</dt>
      <dd>{dir === undefined ? children : <bdi dir={dir}>{children}</bdi>}</dd>
    </div>
  );
}
