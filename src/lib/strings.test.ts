import { afterEach, describe, expect, it } from 'vitest';
import { en } from './i18n/en';
import { plural, ru } from './i18n/ru';
import { formatBytes } from './format';
import { language, setLanguage, storedLanguage, strings } from './strings';

/** Every text path of a strings object. */
function paths(value: unknown, prefix = ''): string[] {
  if (value === null || typeof value !== 'object') return [prefix];
  return Object.entries(value).flatMap(([key, child]) =>
    paths(child, prefix === '' ? key : `${prefix}.${key}`),
  );
}

describe('interface language', () => {
  afterEach(() => {
    setLanguage('en');
  });

  it('is English unless chosen', () => {
    expect(language()).toBe('en');
    expect(strings.nav.settings).toBe('Settings');
  });

  it('switches every reference, including ones taken earlier', () => {
    const t = strings.settings;
    const memoryHint = strings.details.memoryHint;
    setLanguage('ru');
    expect(t.title).toBe('Настройки');
    expect(strings.details.memoryHint(1, 2, 3)).toContain('МиБ');
    expect(memoryHint(1, 2, 3)).toContain('MiB');
    expect(Object.keys(t.themes)).toEqual(['system', 'light', 'dark']);
    expect(document.documentElement.lang).toBe('ru');
    expect(storedLanguage()).toBe('ru');
    setLanguage('en');
    expect(t.title).toBe('Settings');
  });

  it('has the same texts in both languages, all filled in', () => {
    expect(paths(ru)).toEqual(paths(en));
    const empty = paths(ru).filter((p) => {
      const value = p.split('.').reduce<unknown>((v, k) => (v as Record<string, unknown>)[k], ru);
      return typeof value === 'string' && value.trim() === '';
    });
    expect(empty).toEqual([]);
  });

  it('uses Russian plural forms and units', () => {
    expect([1, 2, 5, 11, 21, 22, 25].map((n) => plural(n, 'файл', 'файла', 'файлов'))).toEqual([
      'файл',
      'файла',
      'файлов',
      'файлов',
      'файл',
      'файла',
      'файлов',
    ]);
    expect(formatBytes(1536)).toBe('1.5 KiB');
    setLanguage('ru');
    expect(formatBytes(1536)).toBe('1,5 КиБ');
    expect(formatBytes(3)).toBe('3 байта');
    expect(strings.install.files(3, 21)).toBe('Проверено файлов: 3 из 21');
    expect(strings.runs.dropped(21)).toBe('Более ранние строки не показаны: 21 строка.');
  });
});
