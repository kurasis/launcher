import { describe, expect, it } from 'vitest';
import { dayOf, shortDayOf } from './versions';

describe('dayOf', () => {
  const now = Date.UTC(2026, 9, 23);

  it('names the day, with the year only when it is another one', () => {
    expect(dayOf(Date.UTC(2026, 9, 12, 12) / 1000, now)).toBe('October 12');
    expect(shortDayOf(Date.UTC(2025, 9, 12, 12) / 1000, now)).toBe('Oct 12, 2025');
  });

  it('gives no date for a time the catalog should never send', () => {
    for (const seconds of [0, -5, Number.NaN, Number.POSITIVE_INFINITY, 1e13, 9.2e18]) {
      expect(dayOf(seconds, now)).toBe('');
      expect(shortDayOf(seconds, now)).toBe('');
    }
  });
});
