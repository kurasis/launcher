// The interface text in the chosen language (D-038). Components import `strings`
// and read it while rendering; it always resolves to the current language, also
// through references taken at module load (`const t = strings.settings`).
import type { Language } from './contracts';
import { en } from './i18n/en';
import { ru } from './i18n/ru';

export type { SettingsSection } from './i18n/en';

/** The shape every language provides: English's keys, any text. */
export type Localized<T> = T extends string
  ? string
  : T extends (...args: infer A) => string
    ? (...args: A) => string
    : { readonly [K in keyof T]: Localized<T[K]> };
export type Strings = Localized<typeof en>;

const languages: Record<Language, Strings> = { en, ru };
export const LANGUAGES: Language[] = ['en', 'ru'];

/** Mirrors the setting for the first paint and for the console window. */
const STORAGE_KEY = 'launcher.language';

function isLanguage(value: unknown): value is Language {
  return typeof value === 'string' && value in languages;
}

function stored(): Language {
  try {
    const value = window.localStorage.getItem(STORAGE_KEY);
    return isLanguage(value) ? value : 'en';
  } catch {
    return 'en';
  }
}

let current: Language = stored();

export function language(): Language {
  return current;
}

/** Switches the interface text; the caller re-renders. */
export function setLanguage(next: Language): void {
  if (!isLanguage(next)) return;
  current = next;
  document.documentElement.lang = next;
  try {
    window.localStorage.setItem(STORAGE_KEY, next);
  } catch {
    // Storage is optional; the setting in the core stays authoritative.
  }
}

/** Re-reads the mirrored language (the console window follows the main one). */
export function storedLanguage(): Language {
  return stored();
}

export const LANGUAGE_STORAGE_KEY = STORAGE_KEY;

function lookup(path: readonly PropertyKey[]): unknown {
  let value: unknown = languages[current];
  for (const key of path) {
    if (value === null || typeof value !== 'object') return undefined;
    value = (value as Record<PropertyKey, unknown>)[key];
  }
  return value;
}

const views = new Map<string, object>();

/** A read-only view of the strings at `path` in whichever language is current. */
function view(path: readonly PropertyKey[]): object {
  const id = path.map(String).join('\u0000');
  const cached = views.get(id);
  if (cached !== undefined) return cached;
  const proxy = new Proxy(
    {},
    {
      get(_target, key) {
        const value = lookup([...path, key]);
        return value !== null && typeof value === 'object' ? view([...path, key]) : value;
      },
      has(_target, key) {
        const value = lookup(path);
        return value !== null && typeof value === 'object' && key in value;
      },
      ownKeys() {
        const value = lookup(path);
        return value !== null && typeof value === 'object' ? Reflect.ownKeys(value) : [];
      },
      getOwnPropertyDescriptor(_target, key) {
        const value = lookup(path);
        if (value === null || typeof value !== 'object' || !(key in value)) return undefined;
        return { configurable: true, enumerable: true, value: lookup([...path, key]) };
      },
    },
  );
  views.set(id, proxy);
  return proxy;
}

export const strings = view([]) as Strings;
