import { type FocusEvent, type RefObject, useEffect, useRef } from 'react';

/** What takes the focus in a row: its first control. */
const CONTROL = 'button:not(:disabled), input:not(:disabled), select:not(:disabled), textarea';

/** The focus was dropped: the control that had it went away, and WebView2 left it on the page. */
export function focusLost(): boolean {
  const at = document.activeElement;
  return at === null || at === document.body;
}

/**
 * Keeps the focus in a list whose rows go away under it (a request accepted, a friend
 * removed): the row that comes into the place of the one that had it takes it, else the
 * last row, as long as the focus was in the list. `rows` are what the list shows; spread
 * what this gives on the list.
 */
export function useRowFocus(list: RefObject<HTMLElement | null>, rows: unknown) {
  const at = useRef<number | null>(null);
  useEffect(() => {
    const element = list.current;
    if (element === null || at.current === null || !focusLost()) return;
    const items = [...element.children];
    items[Math.min(at.current, items.length - 1)]?.querySelector<HTMLElement>(CONTROL)?.focus();
  }, [list, rows]);
  return {
    onFocus: (event: FocusEvent<HTMLElement>) => {
      const target = event.target;
      at.current = [...event.currentTarget.children].findIndex((item) => item.contains(target));
    },
    onBlur: (event: FocusEvent<HTMLElement>) => {
      const next = event.relatedTarget;
      if (next instanceof Node && !event.currentTarget.contains(next)) at.current = null;
    },
  };
}
