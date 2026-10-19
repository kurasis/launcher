// The screens of the main window and where each one is reached from.
//
// A new top-level screen is a new `View`, a place in `rowViews` or in
// `menuEntries`, and a text in `strings.nav`. A start view the interface does
// not have yet opens Installations.
import type { AppSnapshot, RememberedView, SettingsView } from '../lib/contracts';

/** Screens of the main window. */
export type View =
  'feed' | 'home' | 'account' | 'settings' | 'backups' | 'screenshots' | 'diagnostics' | 'about';

/** Tabs in the row, in order: the feed (D-067) first. */
export const rowViews: readonly View[] = ['feed', 'home', 'account', 'settings'];

/** Whether a screen can be opened with these settings: the feed only while it is on. */
export function viewShown(view: View, settings: SettingsView | undefined): boolean {
  return view !== 'feed' || settings?.feed.enabled !== false;
}

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
 * version does not have yet, the feed while it is off, or a value the core did
 * not send, opens Installations.
 */
export function startViewOf(snapshot: AppSnapshot): View {
  if (snapshot.activeOperation !== null || snapshot.currentRun !== null) return 'home';
  const { startView, lastView } = snapshot.settings;
  const wanted: string | undefined = startView === 'last' ? lastView : startView;
  const view = views.find((item) => item === wanted);
  return view !== undefined && rememberedView(view) !== null && viewShown(view, snapshot.settings)
    ? view
    : 'home';
}
