import styles from './StateDot.module.css';

export type DotTone = 'success' | 'busy' | 'warning' | 'danger' | 'neutral';

/**
 * A state as words after a small dot in its tone, without a pill: in a table cell or a
 * fact (concept A). The dot is decorative; the words carry the meaning.
 */
export function StateDot({ tone, label }: { tone: DotTone; label: string }) {
  return (
    <span className={styles.state}>
      <span className={[styles.dot, styles[tone]].join(' ')} aria-hidden="true" />
      {label}
    </span>
  );
}

/** The dot alone, beside words that say the state already. */
export function Dot({ tone }: { tone: DotTone }) {
  return <span className={[styles.dot, styles[tone]].join(' ')} aria-hidden="true" />;
}
