import { afterEach, describe, expect, it } from 'vitest';
import english from '../locales/messages/en.json';
import { formatBytes } from './format';
import {
  direction,
  LANGUAGES,
  language,
  loadLanguage,
  localizeError,
  setLanguage,
  storedLanguage,
  strings,
} from './strings';

async function use(tag: string) {
  setLanguage(await loadLanguage(tag));
}

describe('interface language', () => {
  afterEach(() => {
    setLanguage('en');
  });

  it('is English unless chosen, with every language listed', () => {
    expect(language()).toBe('en');
    expect(strings.nav.settings).toBe('Settings');
    expect(LANGUAGES.map((l) => l.tag)).toEqual(['en', 'ar', 'ru']);
    expect(LANGUAGES.find((l) => l.tag === 'ar')).toMatchObject({ name: 'العربية', dir: 'rtl' });
  });

  it('loads a language when chosen and switches earlier references', async () => {
    const t = strings.settings;
    await use('ru');
    expect(language()).toBe('ru');
    expect(t.title).toBe('Настройки');
    expect(Object.keys(t.themes)).toEqual(['system', 'light', 'dark']);
    expect(document.documentElement.lang).toBe('ru');
    expect(storedLanguage()).toBe('ru');
  });

  it('falls back from a region to its language, and to English', async () => {
    await use('ru-RU');
    expect(language()).toBe('ru');
    await use('pt-BR');
    expect(language()).toBe('en');
    expect(strings.nav.settings).toBe('Settings');
  });

  it('sets the writing direction', async () => {
    await use('ar');
    expect(direction()).toBe('rtl');
    expect(document.documentElement.dir).toBe('rtl');
    expect(strings.nav.settings).toBe('الإعدادات');
    await use('en');
    expect(document.documentElement.dir).toBe('ltr');
  });

  it('uses each language’s plural forms and number format', async () => {
    expect(strings.runs.dropped({ count: 1 })).toBe('1 earlier line not shown.');
    expect(strings.install.files({ done: 1200, total: 3961 })).toBe(
      '1,200 of 3,961 files verified',
    );
    expect(formatBytes(1536)).toBe('1.5 KiB');
    expect(formatBytes(1)).toBe('1 byte');
    await use('ru');
    expect([1, 2, 5, 21].map((count) => strings.runs.dropped({ count }))).toEqual([
      'Более ранние строки не показаны: 1 строка.',
      'Более ранние строки не показаны: 2 строки.',
      'Более ранние строки не показаны: 5 строк.',
      'Более ранние строки не показаны: 21 строка.',
    ]);
    expect(formatBytes(1536)).toBe('1,5 КиБ');
    expect(formatBytes(3)).toBe('3 байта');
    expect(strings.runs.seconds({ seconds: 22.75 })).toBe('22,8 с');
    await use('ar');
    expect([0, 1, 2, 3, 11, 100].map((count) => strings.units.bytes({ count }))).toEqual([
      '0 بايت',
      'بايت واحد',
      'بايتان',
      '3 بايتات',
      '11 بايتًا',
      '100 بايت',
    ]);
    expect(strings.details.memoryHint({ min: 1024, max: 13108, recommended: 4096 })).toContain(
      '13108',
    );
  });

  it('translates core messages by template, with their values', async () => {
    const memory = {
      code: 'INVALID_INPUT' as const,
      message: 'Maximum memory must be between 1024 and 13108 MiB on this computer.',
      template: 'Maximum memory must be between {min} and {max} MiB on this computer.',
      params: { min: 1024, max: 13108 },
    };
    const disk = {
      code: 'DISK_FULL' as const,
      message: 'Saving settings: the disk is full. Free some space and try again.',
      template: '{context}: the disk is full. Free some space and try again.',
      params: { context: 'Saving settings' },
    };
    expect(localizeError(memory)).toBe(memory.message);
    await use('ru');
    expect(localizeError(memory)).toBe(
      'Максимум памяти на этом компьютере — от 1024 до 13108 МиБ.',
    );
    expect(localizeError(disk)).toBe(
      'Сохранение настроек: диск заполнен. Освободите место и попробуйте снова.',
    );
    expect(localizeError({ code: 'APP_BUSY', message: 'A message no catalog knows.' })).toBe(
      'Идёт другая операция. Дождитесь её завершения или отмените её.',
    );
    await use('ar');
    expect(localizeError(disk)).toBe('حفظ الإعدادات: القرص ممتلئ. أفرغ بعض المساحة وحاول مجددًا.');
  });

  it('has the generated English catalog in step with the interface', () => {
    expect(Object.keys(english.errors).length).toBeGreaterThan(150);
    expect(english._meta).toEqual({ name: 'English', dir: 'ltr' });
  });
});
