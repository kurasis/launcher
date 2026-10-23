import { useEffect, useId, useLayoutEffect, useRef, useState, type ReactNode } from 'react';
import { createPortal } from 'react-dom';
import styles from './Mods.module.css';

interface DialogProps {
  title: string;
  onClose: () => void;
  children: ReactNode;
  /** Narrow dialogs ask one thing; wide ones list. */
  wide?: boolean;
}

/** The dialogs open now, the newest last: Escape closes only the one on top. */
const openDialogs: symbol[] = [];

/** What takes the focus in a dialog. */
const CONTROL = 'input:not([disabled]), select:not([disabled]), button:not([disabled])';

/** A modal window on the page body; Escape closes it. */
export function Dialog({ title, onClose, children, wide = false }: DialogProps) {
  const titleId = useId();
  const ref = useRef<HTMLDivElement>(null);
  const [me] = useState(() => Symbol('dialog'));
  // The dialog itself holds the focus until a control shows up in it.
  const waiting = useRef(false);
  // A parent's new onClose on each render must not move the focus again.
  const onCloseRef = useRef(onClose);
  useLayoutEffect(() => {
    onCloseRef.current = onClose;
  });
  // The focus goes back to what opened the dialog when it closes.
  useEffect(() => {
    const before = document.activeElement;
    return () => {
      if (before instanceof HTMLElement && before !== document.body && before.isConnected) {
        before.focus({ preventScroll: true });
      }
    };
  }, []);
  useEffect(() => {
    openDialogs.push(me);
    const listener = (event: KeyboardEvent) => {
      if (event.key === 'Escape' && openDialogs.at(-1) === me) onCloseRef.current();
    };
    window.addEventListener('keydown', listener);
    return () => {
      window.removeEventListener('keydown', listener);
      openDialogs.splice(openDialogs.indexOf(me), 1);
    };
  }, [me]);
  // The focus goes in on opening, and again when it is outside: the controls came
  // after a load, or the one it was on went away. Only into the dialog on top.
  useEffect(() => {
    const box = ref.current;
    if (box === null || openDialogs.at(-1) !== me) return;
    const at = document.activeElement;
    if (box.contains(at) && !(at === box && waiting.current)) return;
    const first = box.querySelector<HTMLElement>(CONTROL);
    if (first !== null) {
      waiting.current = false;
      first.focus();
    } else if (at !== box) {
      waiting.current = true;
      box.focus();
    }
  });
  // On the page body: the catalog's container (layout containment) would
  // otherwise contain the fixed overlay.
  return createPortal(
    <div className={styles.backdrop}>
      <div
        ref={ref}
        role="dialog"
        aria-modal="true"
        aria-labelledby={titleId}
        tabIndex={-1}
        className={[styles.dialog, wide && styles.wide].filter(Boolean).join(' ')}
      >
        <h2 id={titleId} className={styles.dialogTitle}>
          {title}
        </h2>
        {children}
      </div>
    </div>,
    document.body,
  );
}
