import { useRef } from 'react';
import { strings } from '../../lib/strings';
import styles from './Accounts.module.css';

const a = strings.accounts;

/**
 * Demo or offline for players without a Minecraft token. Offline is the full
 * game without the account check, open only while a Microsoft account that
 * owns Minecraft is signed in here; the core checks the same at launch.
 * Two radios in one tab stop; the arrow keys move between them.
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
  const buttons = useRef(new Map<boolean, HTMLButtonElement>());
  const choose = (choice: boolean) => {
    if (disabled || (choice && !allowed) || choice === shown) return;
    onChange(choice);
    buttons.current.get(choice)?.focus();
  };
  return (
    <div className={styles.segmented} role="radiogroup" aria-label={a.playMode}>
      {([false, true] as const).map((choice) => (
        <button
          key={String(choice)}
          ref={(node) => {
            if (node === null) buttons.current.delete(choice);
            else buttons.current.set(choice, node);
          }}
          type="button"
          role="radio"
          aria-checked={shown === choice}
          tabIndex={shown === choice ? 0 : -1}
          disabled={disabled || (choice && !allowed)}
          onClick={() => {
            choose(choice);
          }}
          onKeyDown={(event) => {
            if (!['ArrowLeft', 'ArrowRight', 'ArrowUp', 'ArrowDown'].includes(event.key)) return;
            event.preventDefault();
            choose(!choice);
          }}
        >
          {choice ? a.modeOffline : a.modeDemo}
        </button>
      ))}
    </div>
  );
}
