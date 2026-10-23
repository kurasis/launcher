/** A dialog open now: what it holds, and how the focus goes back into it. */
interface OpenDialog {
  readonly id: symbol;
  readonly holds: (node: Node) => boolean;
  readonly refocus: () => void;
}

/** The dialogs open now, the newest last: Escape and Tab belong to the one on top. */
const open: OpenDialog[] = [];

/** The page under the dialogs: out of reach (keyboard, pointer, screen readers) while one is open. */
function syncPage() {
  document.getElementById('root')?.toggleAttribute('inert', open.length > 0);
}

export function addDialog(dialog: OpenDialog) {
  open.push(dialog);
  syncPage();
}

export function removeDialog(id: symbol) {
  const at = open.findIndex((one) => one.id === id);
  if (at !== -1) open.splice(at, 1);
  syncPage();
}

/** Whether an open dialog holds this node: the one on top, or one under it. */
export function inDialog(node: Node): boolean {
  return open.some((one) => one.holds(node));
}

/** The dialog on top, if one is open. */
export function topDialog(): OpenDialog | undefined {
  return open.at(-1);
}

/**
 * Whether a dialog is over this element (a viewer, a menu, a dialog under another):
 * its keys wait, they are the top dialog's. Without an element, whether one is open.
 */
export function coveredByDialog(element: Element | null): boolean {
  const top = open.at(-1);
  return top !== undefined && (element === null || !top.holds(element));
}
