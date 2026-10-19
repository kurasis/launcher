import { strings } from '../../lib/strings';
import styles from './Accounts.module.css';

const a = strings.accounts;

/**
 * Demo or offline for players without a Minecraft token. Offline is the full
 * game without the account check, open only while a Microsoft account that
 * owns Minecraft is signed in here; the core checks the same at launch.
 */
export function PlayModeSwitch({
  offline,
  allowed,
  disabled,
  onChange,
}: {
  offline: boolean;
  allowed: boolean;
  disabled: boolean;
  onChange: (offline: boolean) => void;
}) {
  const shown = offline && allowed;
  return (
    <div className={styles.mode}>
      <div className={styles.segmented} role="radiogroup" aria-label={a.playMode}>
        {([false, true] as const).map((choice) => (
          <button
            key={String(choice)}
            type="button"
            role="radio"
            aria-checked={shown === choice}
            disabled={disabled || (choice && !allowed)}
            onClick={() => {
              if (choice !== shown) onChange(choice);
            }}
          >
            {choice ? a.modeOffline : a.modeDemo}
          </button>
        ))}
      </div>
      <p className={styles.hint}>
        {!allowed ? a.offlineNeedsOwner : shown ? a.offlineHint : a.demoHint}
      </p>
    </div>
  );
}
