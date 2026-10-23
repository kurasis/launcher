import { describe, expect, it } from 'vitest';
import { isoTime, listTime, messageTime } from './chatTime';

/** Unix seconds of a moment on this computer's clock. */
function at(year: number, month: number, day: number, hour: number, minute: number): number {
  return new Date(year, month - 1, day, hour, minute).getTime() / 1000;
}

const NOW = new Date(2026, 9, 23, 20, 15).getTime();

/** ICU puts a narrow space before AM and PM in some versions. */
function plain(text: string): string {
  return text.replace(/\s/gu, ' ');
}

describe('chat times', () => {
  it('says the time today, "yesterday", then the date', () => {
    expect(plain(listTime(at(2026, 10, 23, 19, 41), NOW))).toBe('07:41 PM');
    expect(plain(listTime(at(2026, 10, 23, 0, 5), NOW))).toBe('12:05 AM');
    expect(plain(listTime(at(2026, 10, 22, 23, 59), NOW))).toBe('yesterday');
    expect(plain(listTime(at(2026, 10, 12, 9, 0), NOW))).toBe('Oct 12');
    expect(plain(listTime(at(2025, 12, 31, 9, 0), NOW))).toBe('Dec 31, 2025');
  });

  it('gives a message its time, with the day when it was not today', () => {
    expect(plain(messageTime(at(2026, 10, 23, 19, 40), NOW))).toBe('07:40 PM');
    expect(plain(messageTime(at(2026, 10, 22, 19, 40), NOW))).toBe('Oct 22, 07:40 PM');
    expect(plain(messageTime(at(2025, 1, 2, 8, 5), NOW))).toBe('Jan 2, 2025, 08:05 AM');
  });

  it('takes a clock that is behind as today', () => {
    expect(plain(listTime(at(2026, 10, 24, 1, 0), NOW))).toBe('01:00 AM');
  });

  it('shows nothing for a time no date can hold, and never throws', () => {
    for (const junk of [Number.NaN, Infinity, -Infinity, 9e15, -9e15]) {
      expect(listTime(junk, NOW)).toBe('');
      expect(messageTime(junk, NOW)).toBe('');
      expect(isoTime(junk)).toBeUndefined();
    }
    expect(isoTime(0)).toBe('1970-01-01T00:00:00.000Z');
  });
});
