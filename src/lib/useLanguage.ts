import { useEffect, useState } from 'react';
import { language, loadLanguage, setLanguage } from './strings';

/**
 * Applies the language setting (D-038): loads its catalog, switches, and returns
 * the language now shown, so the caller can re-render when it changes.
 */
export function useLanguage(wanted: string | undefined): string {
  const [shown, setShown] = useState(language);
  useEffect(() => {
    if (wanted === undefined) return;
    let active = true;
    void loadLanguage(wanted)
      .then((tag) => {
        if (!active) return;
        setLanguage(tag);
        setShown(tag);
      })
      // A catalog that fails to load leaves the current language in place.
      .catch(() => undefined);
    return () => {
      active = false;
    };
  }, [wanted]);
  return shown;
}
