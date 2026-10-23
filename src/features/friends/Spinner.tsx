import styles from './Friends.module.css';

/** Something is on its way: a small turning ring and what it waits for. */
export function Spinner({ label }: { label: string }) {
  return (
    <p className={styles.loading} role="status">
      <span className={styles.spinner} aria-hidden="true" />
      {label}
    </p>
  );
}
