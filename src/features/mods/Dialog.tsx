import { useEffect, useId, useLayoutEffect, useRef, type ReactNode } from 'react';
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

/** A modal window on the page body; Escape closes it. */
export function Dialog({ title, onClose, children, wide = false }: DialogProps) {
  const titleId = useId();
  const ref = useRef<HTMLDivElement>(null);
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
    const me = Symbol('dialog');
    openDialogs.push(me);
    const first = ref.current?.querySelector<HTMLElement>('input, select, button');
    first?.focus();
    const listener = (event: KeyboardEvent) => {
      if (event.key === 'Escape' && openDialogs.at(-1) === me) onCloseRef.current();
    };
    window.addEventListener('keydown', listener);
    return () => {
      window.removeEventListener('keydown', listener);
      openDialogs.splice(openDialogs.indexOf(me), 1);
    };
  }, []);
  // On the page body: the catalog's container (layout containment) would
  // otherwise contain the fixed overlay.
  return createPortal(
    <div className={styles.backdrop}>
      <div
        ref={ref}
        role="dialog"
        aria-modal="true"
        aria-labelledby={titleId}
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
