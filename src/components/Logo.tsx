import styles from './Logo.module.css';

/** Product mark: an isometric block. Decorative; the product name is the text next to it. */
export function Logo() {
  return (
    <svg className={styles.logo} viewBox="0 0 32 32" aria-hidden="true" focusable="false">
      <polygon className={styles.top} points="16,3 29,10 16,17 3,10" />
      <polygon className={styles.left} points="3,10 16,17 16,30 3,23" />
      <polygon className={styles.right} points="29,10 16,17 16,30 29,23" />
      <polygon className={styles.shine} points="16,6 23.5,10 16,14 8.5,10" />
    </svg>
  );
}
