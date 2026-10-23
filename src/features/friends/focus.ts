import { type FocusEvent, type RefObject, useEffect, useRef } from 'react';
import { focusLost } from '../../lib/focus';

export { focusLost };

/** What takes the focus in a row: its first control. */
const CONTROL = 'button:not(:disabled), input:not(:disabled), select:not(:disabled), textarea';

/**
 * Keeps the focus in a list whose rows go away under it (a request accepted, a friend
 * removed): when the control that had it is gone with its row, the row that comes into
 * its place takes it, else the last row. The focus leaving the list any other way (to
 * another control, or to nothing with a press on text) is left alone. `rows` are what
 * the list shows; spread what this gives on the list.
 */
export function useRowFocus(list: RefObject<HTMLElement | null>, rows: unknown) {
  const had = useRef<{ index: number; element: Element } | null>(null);
  useEffect(() => {
    const element = list.current;
    const last = had.current;
    if (element === null) had.current = null;
    if (element === null || last === null || last.element.isConnected || !focusLost()) return;
    had.current = null;
    const items = [...element.children];
    items[Math.min(last.index, items.length - 1)]?.querySelector<HTMLElement>(CONTROL)?.focus();
  }, [list, rows]);
  return {
    onFocus: (event: FocusEvent<HTMLElement>) => {
      const target = event.target;
      const index = [...event.currentTarget.children].findIndex((item) => item.contains(target));
      had.current = index === -1 ? null : { index, element: target };
    },
    onBlur: (event: FocusEvent<HTMLElement>) => {
      const next = event.relatedTarget;
      if (next instanceof Node && event.currentTarget.contains(next)) return;
      // Gone to another control, or to nothing while it stays (a press on text): forget
      // it. A control taken out with its row is not here any more and stays remembered.
      const target = event.target;
      queueMicrotask(() => {
        if (had.current?.element === target && (next !== null || target.isConnected)) {
          had.current = null;
        }
      });
    },
  };
}
