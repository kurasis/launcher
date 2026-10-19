// The screens of the main window and where each one is reached from.
//
// A new top-level screen is a new `View`, a place in `rowViews` (the feed goes
// first) or in `menuEntries`, and a text in `strings.nav`. A start view the
// interface does not have yet opens Installations.
import type { AppSnapshot, RememberedView } from '../lib/contracts';

/** Screens of the main window. */
export type View =
  'home' | 'account' | 'settings' | 'backups' | 'screenshots' | 'diagnostics' | 'about';

/** Tabs in the row, in order. */
export const rowViews: readonly View[] = ['home', 'account', 'settings'];

/** An entry of the More menu: a screen, a separator, or the console window. */
export type MenuEntry = View | 'separator' | 'console';

/** The More menu, in order. */
export const menuEntries: readonly MenuEntry[] = [
  'backups',
  'screenshots',
  'separator',
  'diagnostics',
  'console',
  'about',
];

function isMenuView(entry: MenuEntry): entry is View {
  return entry !== 'separator' && entry !== 'console';
}

/** Screens reached from the More menu. */
export const menuViews: readonly View[] = menuEntries.filter(isMenuView);

const views: readonly View[] = [...rowViews, ...menuViews];

/** Start screens that can be remembered for "Last opened"; never menu screens or Settings. */
const remembered: readonly RememberedView[] = ['feed', 'home', 'mods', 'skins', 'account'];

/** `view` as a remembered start screen, or null when it is not one. */
export function rememberedView(view: View): RememberedView | null {
  return remembered.find((item) => item === view) ?? null;
}

/**
 * The screen opened at start: Installations while a game or an install runs,
 * otherwise the chosen start screen (or the last one opened). A screen this
 * version does not have yet, or a value the core did not send, opens
 * Installations.
 */
export function startViewOf(snapshot: AppSnapshot): View {
  if (snapshot.activeOperation !== null || snapshot.currentRun !== null) return 'home';
  const { startView, lastView } = snapshot.settings;
  const wanted: string | undefined = startView === 'last' ? lastView : startView;
  const view = views.find((item) => item === wanted);
  return view !== undefined && rememberedView(view) !== null ? view : 'home';
}
