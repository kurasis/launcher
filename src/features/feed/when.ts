import { numberLocale } from '../../lib/strings';

const UNITS: [Intl.RelativeTimeFormatUnit, number][] = [
  ['year', 365 * 24 * 3600],
  ['month', 30 * 24 * 3600],
  ['week', 7 * 24 * 3600],
  ['day', 24 * 3600],
  ['hour', 3600],
  ['minute', 60],
];

/** "3 hours ago", "in 2 days", in the interface language. */
export function relativeTime(iso: string, now: number = Date.now()): string {
  const time = Date.parse(iso);
  if (Number.isNaN(time)) return '';
  const seconds = Math.round((time - now) / 1000);
  const format = new Intl.RelativeTimeFormat(numberLocale(), { numeric: 'auto' });
  for (const [unit, size] of UNITS) {
    if (Math.abs(seconds) >= size) return format.format(Math.trunc(seconds / size), unit);
  }
  return format.format(0, 'minute');
}

/** A date in the interface language, such as "19 Oct 2026". */
export function shortDate(iso: string): string {
  const time = Date.parse(iso);
  if (Number.isNaN(time)) return '';
  return new Date(time).toLocaleDateString(numberLocale(), { dateStyle: 'medium' });
}

/** A date with its time, such as "25 Oct 2026, 12:00". */
export function dateTime(iso: string): string {
  const time = Date.parse(iso);
  if (Number.isNaN(time)) return '';
  return new Date(time).toLocaleString(numberLocale(), { dateStyle: 'medium', timeStyle: 'short' });
}

const WEEK_MS = 7 * 24 * 3600 * 1000;

/**
 * A card's date: "yesterday", "3 days ago" within a week, then the day and month
 * ("21 October"), with the year when it is not this year's.
 */
export function cardDate(iso: string, now: number = Date.now()): string {
  const time = Date.parse(iso);
  if (Number.isNaN(time)) return '';
  if (Math.abs(now - time) < WEEK_MS) return relativeTime(iso, now);
  const date = new Date(time);
  const thisYear = date.getFullYear() === new Date(now).getFullYear();
  return date.toLocaleDateString(
    numberLocale(),
    thisYear
      ? { day: 'numeric', month: 'long' }
      : { day: 'numeric', month: 'long', year: 'numeric' },
  );
}

/** "now", for something live, in the interface language. */
export function liveNow(): string {
  return new Intl.RelativeTimeFormat(numberLocale(), { numeric: 'auto' }).format(0, 'second');
}

/**
 * A video's length: "4:05", "12:34", or "1:02:05" from an hour, in the digits the
 * interface uses for numbers. Null when the length is unknown.
 */
export function videoLength(seconds: number | null): string | null {
  if (seconds === null || !Number.isFinite(seconds) || seconds < 0) return null;
  const whole = Math.floor(seconds);
  const hours = Math.floor(whole / 3600);
  const minutes = Math.floor((whole % 3600) / 60);
  const rest = whole % 60;
  const plain = new Intl.NumberFormat(numberLocale(), { useGrouping: false });
  const two = new Intl.NumberFormat(numberLocale(), {
    minimumIntegerDigits: 2,
    useGrouping: false,
  });
  return hours > 0
    ? `${plain.format(hours)}:${two.format(minutes)}:${two.format(rest)}`
    : `${plain.format(minutes)}:${two.format(rest)}`;
}

/** A count in the interface's digits, such as "12,345" or "12 345". */
export function count(value: number): string {
  return new Intl.NumberFormat(numberLocale()).format(value);
}
