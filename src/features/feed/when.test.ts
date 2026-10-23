import { afterEach, describe, expect, it } from 'vitest';
import { loadLanguage, setLanguage } from '../../lib/strings';
import { cardDate, count, liveNow, videoLength } from './when';

describe('when', () => {
  afterEach(() => {
    setLanguage('en');
  });

  it('writes a video’s length as m:ss, or h:mm:ss from an hour', () => {
    expect(videoLength(0)).toBe('0:00');
    expect(videoLength(42)).toBe('0:42');
    expect(videoLength(754)).toBe('12:34');
    expect(videoLength(3599)).toBe('59:59');
    expect(videoLength(3600)).toBe('1:00:00');
    expect(videoLength(3725)).toBe('1:02:05');
    expect(videoLength(36_000 + 61.9)).toBe('10:01:01');
    // Unknown (a premiere) or junk: no length.
    expect(videoLength(null)).toBeNull();
    expect(videoLength(-5)).toBeNull();
    expect(videoLength(Number.NaN)).toBeNull();
    expect(videoLength(Number.POSITIVE_INFINITY)).toBeNull();
  });

  it('writes lengths and counts in the digits the interface uses', async () => {
    await loadLanguage('ar');
    setLanguage('ar');
    // Arabic uses Latin digits throughout the launcher (ar-u-nu-latn).
    expect(videoLength(3725)).toBe('1:02:05');
    expect(count(12345)).toMatch(/^12.345$/);
    await loadLanguage('ru');
    setLanguage('ru');
    expect(count(12345)).toBe('12 345');
    expect(liveNow()).toBe('сейчас');
  });

  it('dates a card relatively within a week, then by day and month', () => {
    const now = Date.parse('2026-10-23T12:00:00Z');
    expect(cardDate('2026-10-22T12:00:00Z', now)).toBe('yesterday');
    expect(cardDate('2026-10-20T12:00:00Z', now)).toBe('3 days ago');
    expect(cardDate('2026-10-01T12:00:00Z', now)).toBe('October 1');
    expect(cardDate('2025-10-01T12:00:00Z', now)).toBe('October 1, 2025');
    expect(cardDate('not a date', now)).toBe('');
    expect(liveNow()).toBe('now');
  });
});
