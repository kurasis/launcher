import styles from './Skins.module.css';

/** An on-off switch, drawn as a track with a knob. */
export function Toggle({
  label,
  on,
  disabled,
  onChange,
}: {
  label: string;
  on: boolean;
  disabled?: boolean;
  onChange: (on: boolean) => void;
}) {
  return (
    <button
      type="button"
      role="switch"
      aria-checked={on}
      className={styles.toggle}
      disabled={disabled}
      onClick={() => {
        onChange(!on);
      }}
    >
      <span className={styles.track} aria-hidden="true" />
      {label}
    </button>
  );
}
