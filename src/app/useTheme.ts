import { useEffect } from 'react';
import type { Theme } from '../lib/contracts';

/** Applies the user's theme choice; "system" follows the OS preference. */
export function useTheme(theme: Theme | undefined): void {
  useEffect(() => {
    const root = document.documentElement;
    if (theme === undefined || theme === 'system') {
      delete root.dataset.theme;
    } else {
      root.dataset.theme = theme;
    }
  }, [theme]);
}
