import { type KeyboardEvent, useEffect, useId, useRef, useState } from 'react';
import styles from './App.module.css';

/** One entry of a menu: an item to choose, or a separator between groups. */
export type MenuItem =
  | { kind: 'item'; key: string; label: string; current: boolean; onSelect: () => void }
  | { kind: 'separator'; key: string };

interface MoreMenuProps {
  /** The button's text, e.g. "More" or "More: Screenshots". */
  label: string;
  /** The current screen is one of the menu's. */
  current: boolean;
  entries: readonly MenuItem[];
}

/**
 * A menu button (WAI-ARIA "menu button" pattern) in the main window: Enter,
 * Space, ArrowDown or a click opens it on the first item, ArrowUp on the last;
 * arrows, Home and End move between items; Escape, Tab or a click elsewhere
 * closes it.
 */
export function MoreMenu({ label, current, entries }: MoreMenuProps) {
  const id = useId();
  const buttonId = `${id}-button`;
  const menuId = `${id}-menu`;
  const rootRef = useRef<HTMLDivElement>(null);
  const buttonRef = useRef<HTMLButtonElement>(null);
  const itemRefs = useRef<(HTMLButtonElement | null)[]>([]);
  /** The focused item while open; null while closed. */
  const [active, setActive] = useState<number | null>(null);
  const open = active !== null;
  const items = entries.filter((entry) => entry.kind === 'item');
  const last = items.length - 1;

  useEffect(() => {
    if (active !== null) itemRefs.current[active]?.focus();
  }, [active]);

  const close = (refocus: boolean) => {
    setActive(null);
    if (refocus) buttonRef.current?.focus();
  };

  // A press anywhere outside the menu and its button closes it.
  useEffect(() => {
    if (!open) return;
    const onPointerDown = (event: PointerEvent) => {
      if (event.target instanceof Node && rootRef.current?.contains(event.target)) return;
      setActive(null);
      buttonRef.current?.focus();
    };
    document.addEventListener('pointerdown', onPointerDown, true);
    return () => {
      document.removeEventListener('pointerdown', onPointerDown, true);
    };
  }, [open]);

  const onButtonKeyDown = (event: KeyboardEvent<HTMLButtonElement>) => {
    switch (event.key) {
      case 'Enter':
      case ' ':
      case 'ArrowDown':
        // Handled here so the browser's own click does not toggle it again.
        event.preventDefault();
        setActive(0);
        break;
      case 'ArrowUp':
        event.preventDefault();
        setActive(last);
        break;
      case 'Escape':
        if (open) {
          event.preventDefault();
          close(true);
        }
        break;
    }
  };

  const choose = (index: number) => {
    close(true);
    items[index]?.onSelect();
  };

  const onMenuKeyDown = (event: KeyboardEvent<HTMLDivElement>) => {
    const at = active ?? 0;
    switch (event.key) {
      case 'ArrowDown':
        event.preventDefault();
        setActive(at >= last ? 0 : at + 1);
        break;
      case 'ArrowUp':
        event.preventDefault();
        setActive(at <= 0 ? last : at - 1);
        break;
      case 'Home':
        event.preventDefault();
        setActive(0);
        break;
      case 'End':
        event.preventDefault();
        setActive(last);
        break;
      case 'Enter':
      case ' ':
        event.preventDefault();
        choose(at);
        break;
      case 'Escape':
        event.preventDefault();
        close(true);
        break;
      case 'Tab':
        // Tab moves on from the button, as if the menu had not been open.
        close(true);
        break;
    }
  };

  return (
    <div className={styles.menuRoot} ref={rootRef}>
      <button
        ref={buttonRef}
        id={buttonId}
        type="button"
        className={styles.navItem}
        aria-haspopup="menu"
        aria-expanded={open}
        aria-controls={open ? menuId : undefined}
        aria-current={current ? 'page' : undefined}
        onClick={() => {
          if (open) close(false);
          else setActive(0);
        }}
        onKeyDown={onButtonKeyDown}
        onKeyUp={(event) => {
          // Space clicks on release; it already opened the menu on press.
          if (event.key === ' ') event.preventDefault();
        }}
      >
        {label}
        <span className={styles.caret} aria-hidden="true">
          ▾
        </span>
      </button>
      {open && (
        <div
          id={menuId}
          role="menu"
          aria-labelledby={buttonId}
          className={styles.menu}
          onKeyDown={onMenuKeyDown}
          onKeyUp={(event) => {
            if (event.key === ' ') event.preventDefault();
          }}
        >
          {entries.map((entry) => {
            if (entry.kind === 'separator') {
              return <div key={entry.key} role="separator" className={styles.menuSeparator} />;
            }
            const at = items.indexOf(entry);
            return (
              <button
                key={entry.key}
                ref={(node) => {
                  itemRefs.current[at] = node;
                }}
                type="button"
                role="menuitem"
                tabIndex={-1}
                className={styles.menuItem}
                aria-current={entry.current ? 'page' : undefined}
                onClick={() => {
                  choose(at);
                }}
                onPointerEnter={() => {
                  setActive(at);
                }}
              >
                {entry.label}
              </button>
            );
          })}
        </div>
      )}
    </div>
  );
}
