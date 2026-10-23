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
  pending = false,
  onChange,
}: {
  offline: boolean;
  allowed: boolean;
  /** The settings cannot be written. */
  disabled: boolean;
  /** A change is being saved: the radios wait, but keep the focus (a disabled
   * control loses it in WebView2 and WebKit). */
  pending?: boolean;
  onChange: (offline: boolean) => void;
}) {
  const shown = offline && allowed;
  const buttons = useRef(new Map<boolean, HTMLButtonElement>());
  const choose = (choice: boolean) => {
    if (disabled || pending || (choice && !allowed) || choice === shown) return;
    onChange(choice);
    buttons.current.get(choice)?.focus();
  };
  return (
    <div
      className={styles.segmented}
      role="radiogroup"
      aria-label={a.playMode}
      aria-busy={pending || undefined}
    >
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
          aria-disabled={pending || undefined}
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
