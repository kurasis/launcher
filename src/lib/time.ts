import { numberLocale } from './strings';

const DAY_MS = 86_400_000;

/** Calendar days from `at` to `now` in local time: 0 today, 1 yesterday; never below 0. */
export function calendarDays(at: number, now = Date.now()): number {
  const day = (time: number) => {
    const date = new Date(time);
    return Date.UTC(date.getFullYear(), date.getMonth(), date.getDate()) / DAY_MS;
  };
  return Math.max(0, Math.round(day(now) - day(at)));
}

/** "today", "yesterday", "3 days ago": by calendar day, in the interface language. */
export function daysAgo(at: number, now = Date.now()): string {
  return new Intl.RelativeTimeFormat(numberLocale(), { numeric: 'auto' }).format(
    -calendarDays(at, now),
    'day',
  );
}
