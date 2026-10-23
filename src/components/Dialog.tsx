import {
  useCallback,
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
import { addDialog, inDialog, removeDialog, topDialog } from './dialogStack';

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
  // What had the focus, taken once on opening: StrictMode runs the effects twice, the
  // second time with the focus already inside.
  const [opener] = useState(() => document.activeElement);
  // The dialog itself holds the focus until a control shows up in it.
  const waiting = useRef(false);
  // A parent's new onClose on each render must not move the focus again.
  const onCloseRef = useRef(onClose);
  const closableRef = useRef(closable);
  const initialRef = useRef(initialFocus);
  useLayoutEffect(() => {
    onCloseRef.current = onClose;
    closableRef.current = closable;
    initialRef.current = initialFocus;
  });
  // Where the focus goes in, on opening and each time it is outside: what was asked
  // for while it is there, else the first control, else the dialog until one shows up.
  const refocus = useCallback(() => {
    const box = ref.current;
    if (box === null) return;
    const wanted = initialRef.current;
    const chosen = wanted === 'dialog' ? box : wanted?.current;
    const target =
      chosen instanceof HTMLElement && box.contains(chosen) && !chosen.matches(':disabled')
        ? chosen
        : box.querySelector<HTMLElement>(CONTROL);
    waiting.current = target === null;
    (target ?? box).focus();
  }, []);
  useEffect(() => {
    addDialog({ id: me, holds: (node) => ref.current?.contains(node) ?? false, refocus });
    const onKey = (event: globalThis.KeyboardEvent) => {
      // An inner control used it (a menu), or an IME drops what is being composed.
      if (event.key !== 'Escape' || event.defaultPrevented || event.isComposing) return;
      if (topDialog()?.id === me && closableRef.current) onCloseRef.current();
    };
    // The focus that lands outside the dialogs (a viewer over the page) comes back to
    // the one on top. Into one under it only from code: an inner one, closing, put it there.
    const onFocus = (event: FocusEvent) => {
      if (topDialog()?.id !== me || !(event.target instanceof Node)) return;
      if (!inDialog(event.target)) refocus();
    };
    window.addEventListener('keydown', onKey);
    document.addEventListener('focusin', onFocus, true);
    return () => {
      window.removeEventListener('keydown', onKey);
      document.removeEventListener('focusin', onFocus, true);
      removeDialog(me);
      // The focus goes back to what opened the dialog, unless the one who closed it
      // has put it somewhere. When that is gone, the dialog under it or the page takes it.
      const lost = () =>
        document.activeElement === null || document.activeElement === document.body;
      if (!lost()) return;
      if (opener instanceof HTMLElement && opener !== document.body && opener.isConnected) {
        opener.focus({ preventScroll: true });
      }
      if (!lost()) return;
      const below = topDialog();
      if (below !== undefined) below.refocus();
      else
        document.querySelector<HTMLElement>('main[tabindex="-1"]')?.focus({ preventScroll: true });
    };
  }, [me, opener, refocus]);
  // The focus goes in on opening, and again when it is outside: the controls came
  // after a load, or the one it was on went away. Only into the dialog on top.
  useEffect(() => {
    const box = ref.current;
    if (box === null || topDialog()?.id !== me) return;
    const at = document.activeElement;
    if (box.contains(at) && !(at === box && waiting.current)) return;
    refocus();
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
    <div
      className={styles.backdrop}
      onMouseDown={(event) => {
        // A press on the scrim leaves the focus where it is.
        if (event.target === event.currentTarget) event.preventDefault();
      }}
    >
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
