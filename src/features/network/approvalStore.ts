import { useSyncExternalStore } from 'react';

/** Operations whose question the player closed without answering, newest last (D-051). */
const aside: string[] = [];
/** Kept at most: one per operation, and operations run one at a time. */
const KEPT = 32;
const listeners = new Set<() => void>();

function changed() {
  for (const listener of listeners) listener();
}

function subscribe(listener: () => void) {
  listeners.add(listener);
  return () => {
    listeners.delete(listener);
  };
}

/** The question waits unanswered; the operation waits with it. */
export function putAside(operationId: string) {
  if (aside.includes(operationId)) return;
  aside.push(operationId);
  if (aside.length > KEPT) aside.shift();
  changed();
}

/** The question shows again. */
export function bringBack(operationId: string) {
  const at = aside.indexOf(operationId);
  if (at === -1) return;
  aside.splice(at, 1);
  changed();
}

/** Whether this operation's question was put aside. */
export function useAside(operationId: string | null): boolean {
  return useSyncExternalStore(subscribe, () => operationId !== null && aside.includes(operationId));
}
