import type { ReviewKind } from '../../lib/contracts';

/** What a screen knows when it asks for a review (D-070). */
export interface ReviewPrefill {
  kind?: ReviewKind;
  category?: string;
  subcategory?: string;
  instanceId?: string | null;
  /** A game run whose files are the logs to attach. */
  runId?: string | null;
  /** The launcher failure the review is about. */
  diagnosticId?: string | null;
  /** The server ad complained about. */
  ad?: number | null;
}

const EVENT = 'launcher:open-review';

/** Opens the review screen from anywhere, with what the caller knows. */
export function openReview(prefill: ReviewPrefill = {}): void {
  window.dispatchEvent(new CustomEvent<ReviewPrefill>(EVENT, { detail: prefill }));
}

/** Calls `handler` whenever a screen asks for a review; returns the unsubscribe. */
export function onOpenReview(handler: (prefill: ReviewPrefill) => void): () => void {
  const listener = (event: Event) => {
    handler((event as CustomEvent<ReviewPrefill>).detail);
  };
  window.addEventListener(EVENT, listener);
  return () => {
    window.removeEventListener(EVENT, listener);
  };
}
