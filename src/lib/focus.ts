/** The focus was dropped: the control that had it went away, and WebView2 left it on the page. */
export function focusLost(): boolean {
  const at = document.activeElement;
  return at === null || at === document.body;
}
