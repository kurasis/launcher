import type { ReactNode } from 'react';
import styles from './Tag.module.css';

export type TagTone = 'success' | 'warning' | 'outline' | 'neutral';

/**
 * A short fact as a pill beside a name (concept A): "Open: 2 of 8 playing" in the
 * success tint, "Update: 0.6.7" in the warning tint, "Closed" or "Owner" outlined, a
 * mod's name plain. One size for all,
 * no dot; a long one wraps. The words carry the meaning, the tone only helps.
 */
export function Tag({ tone = 'outline', children }: { tone?: TagTone; children: ReactNode }) {
  return <span className={[styles.tag, styles[tone]].join(' ')}>{children}</span>;
}
