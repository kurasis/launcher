import type { Theme } from './contracts';

/** Mirrors the setting for the first paint and for the console window, which cannot
 * read settings (as the language does, D-038). */
export const THEME_STORAGE_KEY = 'launcher.theme';

/** Shows a theme: "system" follows the OS preference (tokens.css). */
export function applyTheme(theme: Theme): void {
  const root = document.documentElement;
  if (theme === 'system') {
    delete root.dataset.theme;
  } else {
    root.dataset.theme = theme;
  }
}

/** The last theme chosen in Settings, "system" if none was kept. */
export function storedTheme(): Theme {
  try {
    const value = window.localStorage.getItem(THEME_STORAGE_KEY);
    return value === 'light' || value === 'dark' ? value : 'system';
  } catch {
    return 'system';
  }
}

export function storeTheme(theme: Theme): void {
  try {
    window.localStorage.setItem(THEME_STORAGE_KEY, theme);
  } catch {
    // Storage is optional; the setting in the core stays authoritative.
  }
}
