import { afterEach, describe, expect, it } from 'vitest';
import { setLanguage } from '../strings';
import {
  localizeError,
  localizeMessage,
  ruDownloads,
  ruIoContexts,
  ruMessages,
  ruPatterns,
} from './errors';

// Every Rust source outside tests, as text.
const sources = import.meta.glob<string>('../../../src-tauri/src/**/*.rs', {
  query: '?raw',
  import: 'default',
  eager: true,
});

/** A Rust string literal's value: `\` + newline skips the next line's indent. */
function rust(raw: string): string {
  return raw
    .replace(/\\\n\s*/g, '')
    .replace(/\\"/g, '"')
    .replace(/\\'/g, "'")
    .replace(/\\n/g, '\n');
}

const literal = String.raw`"((?:[^"\\]|\\.|\\\n)*)"`;

function collect(
  pattern: RegExp,
  group: number,
  keep: (m: RegExpExecArray) => boolean = () => true,
): string[] {
  const found = new Set<string>();
  for (const [path, text] of Object.entries(sources)) {
    if (path.endsWith('tests.rs') || path.includes('/tests/')) continue;
    const code = text.split('#[cfg(test)]')[0] ?? '';
    for (const match of code.matchAll(pattern)) {
      if (keep(match)) found.add(rust(match[group] ?? ''));
    }
  }
  return [...found].sort();
}

const errorCall = new RegExp(
  String.raw`LauncherError::(?:new|invalid_input|internal)\(\s*(?:ErrorCode::\w+,\s*)?(format!\(\s*)?` +
    literal,
  'g',
);
const staticMessages = collect(errorCall, 2, (m) => m[1] === undefined);
const formattedMessages = collect(errorCall, 2, (m) => m[1] !== undefined);
const issueCall = new RegExp(String.raw`message:\s*(format!\(\s*)?` + literal, 'g');
const staticIssues = collect(issueCall, 2, (m) => m[1] === undefined);
const formattedIssues = collect(issueCall, 2, (m) => m[1] !== undefined);
const ioContexts = collect(new RegExp(String.raw`from_io\(\s*&?\w+,\s*` + literal, 'g'), 1);

/** A formatted message with sample values in its `{}` holes. */
function sample(message: string): string {
  let n = 0;
  return message.replace(/\{[^}]*\}/g, () => {
    n += 1;
    return message.includes("character '{") ? ';' : String(100 + n);
  });
}

describe('core messages in Russian', () => {
  afterEach(() => {
    setLanguage('en');
  });

  it('found the Rust messages', () => {
    expect(staticMessages.length).toBeGreaterThan(100);
    expect(formattedMessages.length).toBeGreaterThan(5);
    expect(ioContexts.length).toBeGreaterThan(20);
  });

  it('has a translation for every fixed message and notice', () => {
    const missing = [...staticMessages, ...staticIssues].filter((m) => !(m in ruMessages));
    expect(missing).toEqual([]);
  });

  it('has a pattern for every message with values', () => {
    const missing = [...formattedMessages, ...formattedIssues]
      .map(sample)
      .filter((m) => !ruPatterns.some(([pattern]) => pattern.test(m)));
    expect(missing).toEqual([]);
  });

  it('names every file-operation context', () => {
    expect(ioContexts.filter((c) => !(c in ruIoContexts))).toEqual([]);
    expect(Object.keys(ruDownloads)).toContain('Minecraft metadata');
  });

  it('translates only in Russian, with values and a code fallback', () => {
    const error = {
      code: 'INVALID_INPUT' as const,
      message: 'Maximum memory must be between 1024 and 13108 MiB on this computer.',
    };
    expect(localizeError(error)).toBe(error.message);
    setLanguage('ru');
    expect(localizeError(error)).toBe('Максимум памяти на этом компьютере — от 1024 до 13108 МиБ.');
    expect(
      localizeMessage('Saving settings: the disk is full. Free some space and try again.'),
    ).toBe('Сохранение настроек: диск заполнен. Освободите место и попробуйте снова.');
    expect(
      localizeError({
        code: 'NETWORK_ERROR',
        message: 'Downloading Minecraft metadata failed (HTTP 503).',
      }),
    ).toBe('Не удалось загрузить описание Minecraft (HTTP 503).');
    expect(localizeError({ code: 'DISK_FULL', message: 'Something new.' })).toBe(
      'Диск заполнен. Освободите место и попробуйте снова.',
    );
  });
});
