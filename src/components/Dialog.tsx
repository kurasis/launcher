import {
  useEffect,
  useId,
  useLayoutEffect,
  useRef,
  useState,
  type KeyboardEvent,
  type ReactNode,
  type RefObject,
} from 'react';
import { createPortal } from 'react-dom';
import styles from './Dialog.module.css';

interface DialogProps {
  title: string;
  onClose: () => void;
  children: ReactNode;
  /** Narrow dialogs ask one thing; wide ones list. */
  wide?: boolean;
  /** The id of the text that says what the dialog asks. */
  describedBy?: string | undefined;
  /**
   * What takes the focus when it opens: a control (the safe choice of a question), or
   * the dialog itself when it opened by itself and no answer should come from a stray
   * Enter. By default the first control.
   */
  initialFocus?: RefObject<HTMLElement | null> | 'dialog' | undefined;
  /** Escape closes it. False while something runs that must not be left. */
  closable?: boolean;
}

/** The dialogs open now, the newest last: Escape closes only the one on top. */
const openDialogs: symbol[] = [];

/** What takes the focus in a dialog: not a control disabled itself or by its fieldset. */
const CONTROL =
  'input:not(:disabled), select:not(:disabled), textarea:not(:disabled), button:not(:disabled)';

/** What Tab stops on. */
const TABBABLE = [
  'a[href]',
  'button:not(:disabled)',
  'input:not(:disabled):not([type="hidden"])',
  'select:not(:disabled)',
  'textarea:not(:disabled)',
  'summary',
  '[tabindex]:not([tabindex="-1"])',
].join(', ');

/** The page under the dialogs: out of reach (keyboard, pointer, screen readers) while one is open. */
function syncPage() {
  document.getElementById('root')?.toggleAttribute('inert', openDialogs.length > 0);
}

/** The controls Tab stops on, in order; not those folded away in a closed details. */
function stops(box: HTMLElement): HTMLElement[] {
  return [...box.querySelectorAll<HTMLElement>(TABBABLE)].filter((element) => {
    const folded = element.closest('details:not([open])');
    return folded === null || (element.tagName === 'SUMMARY' && element.parentElement === folded);
  });
}

/** A modal window on the page body; Escape closes it, Tab stays in it. */
export function Dialog({
  title,
  onClose,
  children,
  wide = false,
  describedBy,
  initialFocus,
  closable = true,
}: DialogProps) {
  const titleId = useId();
  const ref = useRef<HTMLDivElement>(null);
  const [me] = useState(() => Symbol('dialog'));
  // The dialog itself holds the focus until a control shows up in it.
  const waiting = useRef(false);
  const opened = useRef(false);
  // A parent's new onClose on each render must not move the focus again.
  const onCloseRef = useRef(onClose);
  const closableRef = useRef(closable);
  useLayoutEffect(() => {
    onCloseRef.current = onClose;
    closableRef.current = closable;
  });
  useEffect(() => {
    const before = document.activeElement;
    openDialogs.push(me);
    syncPage();
    const listener = (event: globalThis.KeyboardEvent) => {
      // An inner control used it (a menu), or an IME drops what is being composed.
      if (event.key !== 'Escape' || event.defaultPrevented || event.isComposing) return;
      if (openDialogs.at(-1) === me && closableRef.current) onCloseRef.current();
    };
    window.addEventListener('keydown', listener);
    return () => {
      window.removeEventListener('keydown', listener);
      openDialogs.splice(openDialogs.indexOf(me), 1);
      syncPage();
      // The focus goes back to what opened the dialog, unless the one who closed it
      // has put it somewhere. When that is gone, the page takes it.
      const now = document.activeElement;
      if (now !== null && now !== document.body) return;
      if (before instanceof HTMLElement && before !== document.body && before.isConnected) {
        before.focus({ preventScroll: true });
      } else {
        document.querySelector<HTMLElement>('main[tabindex="-1"]')?.focus({ preventScroll: true });
      }
    };
  }, [me]);
  // The focus goes in on opening, and again when it is outside: the controls came
  // after a load, or the one it was on went away. Only into the dialog on top.
  useEffect(() => {
    const box = ref.current;
    if (box === null || openDialogs.at(-1) !== me) return;
    if (!opened.current) {
      opened.current = true;
      if (initialFocus === 'dialog') {
        box.focus();
        return;
      }
      const chosen = initialFocus?.current;
      if (chosen instanceof HTMLElement && chosen.isConnected && !chosen.matches(':disabled')) {
        chosen.focus();
        return;
      }
    }
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

  // Tab and Shift+Tab go round the dialog's controls, never to the page under it.
  const onKeyDown = (event: KeyboardEvent<HTMLDivElement>) => {
    const box = ref.current;
    if (event.key !== 'Tab' || box === null) return;
    const all = stops(box);
    const first = all[0];
    const last = all.at(-1);
    const at = document.activeElement;
    if (first === undefined || last === undefined) {
      event.preventDefault();
    } else if (event.shiftKey && (at === first || at === box || !box.contains(at))) {
      event.preventDefault();
      last.focus();
    } else if (!event.shiftKey && (at === last || !box.contains(at))) {
      event.preventDefault();
      first.focus();
    }
  };

  // On the page body: containers with layout containment and the rail's stacking
  // context would otherwise hold the fixed overlay.
  return createPortal(
    <div className={styles.backdrop}>
      <div
        ref={ref}
        role="dialog"
        aria-modal="true"
        aria-labelledby={titleId}
        aria-describedby={describedBy}
        tabIndex={-1}
        className={[styles.dialog, wide && styles.wide].filter(Boolean).join(' ')}
        onKeyDown={onKeyDown}
      >
        <h2 id={titleId} className={styles.title}>
          {title}
        </h2>
        {children}
      </div>
    </div>,
    document.body,
  );
}
