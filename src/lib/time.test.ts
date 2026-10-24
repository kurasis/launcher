import { describe, expect, it } from 'vitest';
import { shortWhen } from './time';

// Local times, so the calendar days hold in any time zone.
const now = new Date(2026, 9, 24, 12, 0).getTime();
const at = (...parts: [number, number, number, number, number]) => new Date(...parts).toISOString();
// ICU versions differ in the space before AM and PM.
const when = (...args: Parameters<typeof shortWhen>) => shortWhen(...args).replace(/\s/gu, ' ');

describe('shortWhen', () => {
  it('names today and yesterday with the time', () => {
    expect(when(at(2026, 9, 24, 9, 5), {}, now)).toBe('today, 9:05 AM');
    expect(when(at(2026, 9, 23, 19, 40), { capital: true }, now)).toBe('Yesterday, 7:40 PM');
  });

  it('gives the date alone before, the time only when asked, the year when not this one', () => {
    expect(when(at(2026, 9, 19, 8, 0), {}, now)).toBe('October 19');
    // The word between the date and the time depends on the ICU version.
    expect(when(at(2026, 9, 3, 18, 2), { time: 'always' }, now)).toMatch(/^October 3\D+6:02 PM$/);
    expect(when(at(2025, 11, 31, 23, 0), {}, now)).toBe('December 31, 2025');
  });

  it('says nothing for a date that does not parse', () => {
    expect(when('not a date', {}, now)).toBe('');
  });
});
