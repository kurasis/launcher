import { useEffect } from 'react';
import type { Theme } from '../lib/contracts';
import { applyTheme, storeTheme } from '../lib/theme';

/** Applies the user's theme choice; "system" follows the OS preference. Until the
 * settings arrive the theme kept from last time stays (main.tsx). */
export function useTheme(theme: Theme | undefined): void {
  useEffect(() => {
    if (theme === undefined) return;
    applyTheme(theme);
    storeTheme(theme);
  }, [theme]);
}
