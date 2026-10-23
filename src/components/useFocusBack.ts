import { type RefObject, useEffect, useRef } from 'react';
import { focusLost } from '../lib/focus';

/**
 * Hands the focus on when what had it went away: each time `state` changes (a question
 * in place closes, a copy starts or ends) and the focus was dropped to the page, it goes
 * to the first of `targets` that is shown and can take it. Focus kept elsewhere stays.
 */
export function useFocusBack(state: unknown, ...targets: RefObject<HTMLElement | null>[]) {
  const last = useRef(state);
  useEffect(() => {
    if (Object.is(last.current, state)) return;
    last.current = state;
    if (!focusLost()) return;
    for (const target of targets) {
      const element = target.current;
      if (element === null || !element.isConnected) continue;
      element.focus();
      if (document.activeElement === element) return;
    }
  });
}
