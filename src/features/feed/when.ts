import { numberLocale } from '../../lib/strings';

const made = new Map<string, unknown>();

/**
 * The formatter `make` gives for the interface's language, made once per `kind`
 * and language: making one costs far more than using it, and every card asks.
 */
function once<T>(kind: string, make: (locale: string) => T): T {
  const locale = numberLocale();
  const key = `${kind}\u0000${locale}`;
  let found = made.get(key) as T | undefined;
  if (found === undefined) {
    found = make(locale);
    made.set(key, found);
  }
  return found;
}

const relative = () =>
  once('relative', (locale) => new Intl.RelativeTimeFormat(locale, { numeric: 'auto' }));
const dates = (kind: string, options: Intl.DateTimeFormatOptions) =>
  once(kind, (locale) => new Intl.DateTimeFormat(locale, options));
const numbers = (kind: string, options?: Intl.NumberFormatOptions) =>
  once(kind, (locale) => new Intl.NumberFormat(locale, options));

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
  const format = relative();
  for (const [unit, size] of UNITS) {
    if (Math.abs(seconds) >= size) return format.format(Math.trunc(seconds / size), unit);
  }
  return format.format(0, 'minute');
}

/** A date in the interface language, such as "19 Oct 2026". */
export function shortDate(iso: string): string {
  const time = Date.parse(iso);
  if (Number.isNaN(time)) return '';
  return dates('date', { dateStyle: 'medium' }).format(time);
}

/** A date with its time, such as "25 Oct 2026, 12:00". */
export function dateTime(iso: string): string {
  const time = Date.parse(iso);
  if (Number.isNaN(time)) return '';
  return dates('dateTime', { dateStyle: 'medium', timeStyle: 'short' }).format(time);
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
  return (
    thisYear
      ? dates('day', { day: 'numeric', month: 'long' })
      : dates('dayYear', { day: 'numeric', month: 'long', year: 'numeric' })
  ).format(date);
}

/** "now", for something live, in the interface language. */
export function liveNow(): string {
  return relative().format(0, 'second');
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
  const plain = numbers('plain', { useGrouping: false });
  const two = numbers('two', { minimumIntegerDigits: 2, useGrouping: false });
  return hours > 0
    ? `${plain.format(hours)}:${two.format(minutes)}:${two.format(rest)}`
    : `${plain.format(minutes)}:${two.format(rest)}`;
}

/** A count in the interface's digits, such as "12,345" or "12 345". */
export function count(value: number): string {
  return numbers('count').format(value);
}
